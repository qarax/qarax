#!/usr/bin/env bash
# Register the qarax-node as a host and set it to UP.
# Required for VM scheduling (the control plane picks a host in UP state).
#
# Looks up hosts by address so stale data from previous runs doesn't cause failures.
# Usage: ./setup_host.sh [QARAX_URL] [QARAX_NODE_HOST] [QARAX_NODE_PORT]

set -e

QARAX_URL="${1:-http://localhost:8000}"
DEFAULT_QARAX_NODE_HOST="qarax-node"
QARAX_NODE_HOST="${2:-$DEFAULT_QARAX_NODE_HOST}"
QARAX_NODE_PORT="${3:-50051}"
QARAX_NODE_HOST_EXPLICIT=0
if [ $# -ge 2 ]; then
  QARAX_NODE_HOST_EXPLICIT=1
fi

lookup_hosts() {
  curl -fsS "${QARAX_URL}/hosts"
}

select_existing_host() {
  lookup_hosts | python3 -c "
import json, os, sys

configured = os.environ['QARAX_NODE_HOST']
default = os.environ['DEFAULT_QARAX_NODE_HOST']
explicit = os.environ['QARAX_NODE_HOST_EXPLICIT'] == '1'
hosts = json.load(sys.stdin)

def find_by_address(address):
    for host in hosts:
        if host.get('address') == address:
            return host
    return None

def initialized(host):
    return bool(host.get('cloud_hypervisor_version')) or host.get('total_cpus') is not None

selected = find_by_address(configured)
stale = None

if not explicit and configured == default:
    default_host = find_by_address(default)
    initialized_host = next(
        (
            host
            for host in hosts
            if host.get('address') != default and initialized(host)
        ),
        None,
    )
    if initialized_host and (default_host is None or not initialized(default_host)):
        selected = initialized_host
        stale = default_host

if selected:
    print(f\"{selected['id']}\t{selected['address']}\t{stale['id'] if stale else ''}\")
else:
    print(f\"\t{configured}\t{stale['id'] if stale else ''}\")
"
}

set_host_status() {
  local host_id="$1"
  local status="$2"
  curl -s -X PATCH "${QARAX_URL}/hosts/${host_id}" \
    -f \
    -sS \
    -H "Content-Type: application/json" \
    -d "{\"status\":\"${status}\"}" \
    -o /dev/null
}

IFS='	' read -r host_id effective_host stale_host_id <<EOF
$(QARAX_NODE_HOST="${QARAX_NODE_HOST}" DEFAULT_QARAX_NODE_HOST="${DEFAULT_QARAX_NODE_HOST}" QARAX_NODE_HOST_EXPLICIT="${QARAX_NODE_HOST_EXPLICIT}" select_existing_host)
EOF

echo "Registering host (${effective_host}:${QARAX_NODE_PORT})..."

# Attempt registration; ignore errors since the host may already exist with a
# different name (the address column has a UNIQUE constraint).
if [ -z "$host_id" ]; then
  curl -s -X POST "${QARAX_URL}/hosts" \
    -f \
    -sS \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"e2e-node\",\"address\":\"${effective_host}\",\"port\":${QARAX_NODE_PORT},\"host_user\":\"root\",\"password\":\"\"}" \
    -o /dev/null || true
fi

# Find the host by address (name may differ from previous runs)
host_id=$(lookup_hosts | python3 -c "
import sys, json
address = sys.argv[1]
for h in json.load(sys.stdin):
    if h.get('address') == address:
        print(h['id'])
        break
" "${effective_host}")

if [ -z "$host_id" ]; then
  echo "ERROR: Could not find a host with address '${effective_host}'" >&2
  lookup_hosts >&2
  exit 1
fi

curl -fsS -X POST "${QARAX_URL}/hosts/${host_id}/init" -o /dev/null

if [ -n "$stale_host_id" ]; then
  set_host_status "$stale_host_id" "down"
  echo "Marked stale host DOWN (id: ${stale_host_id})"
fi

echo "Host initialized and set to UP (id: ${host_id})"
