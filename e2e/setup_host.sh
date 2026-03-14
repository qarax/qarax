#!/usr/bin/env bash
# Register the configured qarax-node host and initialize it.
# Required for VM scheduling (the control plane picks a host in UP state).
#
# In VM mode, pass the VM IP explicitly as QARAX_NODE_HOST or as the second arg.
# Usage: ./setup_host.sh [QARAX_URL] [QARAX_NODE_HOST] [QARAX_NODE_PORT]

set -e

QARAX_URL="${1:-http://localhost:8000}"
QARAX_NODE_HOST="${2:-qarax-node}"
QARAX_NODE_PORT="${3:-50051}"

lookup_hosts() {
  curl -fsS "${QARAX_URL}/hosts"
}

echo "Registering host (${QARAX_NODE_HOST}:${QARAX_NODE_PORT})..."

# Attempt registration; ignore errors since the host may already exist with a
# different name (the address column has a UNIQUE constraint).
host_id=""
host_id="$(lookup_hosts | python3 -c "
import sys, json
address = sys.argv[1]
for h in json.load(sys.stdin):
    if h.get('address') == address:
        print(h['id'])
        break
" "${QARAX_NODE_HOST}")"

if [ -z "$host_id" ]; then
  curl -s -X POST "${QARAX_URL}/hosts" \
    -f \
    -sS \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"e2e-node\",\"address\":\"${QARAX_NODE_HOST}\",\"port\":${QARAX_NODE_PORT},\"host_user\":\"root\",\"password\":\"\"}" \
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
" "${QARAX_NODE_HOST}")

if [ -z "$host_id" ]; then
  echo "ERROR: Could not find a host with address '${QARAX_NODE_HOST}'" >&2
  lookup_hosts >&2
  exit 1
fi

curl -fsS -X POST "${QARAX_URL}/hosts/${host_id}/init" -o /dev/null

echo "Host initialized and set to UP (id: ${host_id})"
