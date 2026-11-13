#!/usr/bin/env bash
# Register the configured qarax-node host and initialize it.
# Required for VM scheduling (the control plane picks a host in UP state).
#
# In VM mode, pass the VM IP explicitly as QARAX_NODE_HOST or as the second arg.
# Usage: ./setup_host.sh [QARAX_URL] [QARAX_NODE_HOST] [QARAX_NODE_PORT] [HOST_NAME]

set -euo pipefail

QARAX_URL="${1:-http://localhost:8000}"
QARAX_NODE_HOST="${2:-qarax-node}"
QARAX_NODE_PORT="${3:-50051}"
HOST_NAME="${4:-e2e-node}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# find_qarax_bin prefers the newest repo build over a `qarax` on PATH, which can
# be older than the server it talks to.
source "${REPO_ROOT}/demos/lib.sh"

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI binary not found. Build it with: cargo build -p cli"
SERVER="$QARAX_URL"

resolve_existing_host_name() {
	qarax host list -o json 2>/dev/null | python3 -c '
import json, sys
wanted_name, wanted_address, wanted_port = sys.argv[1], sys.argv[2], int(sys.argv[3])
for host in json.load(sys.stdin):
    if host.get("name") == wanted_name:
        print(wanted_name)
        raise SystemExit(0)
    if host.get("address") == wanted_address and int(host.get("port", 0)) == wanted_port:
        print(host.get("name", ""))
        raise SystemExit(0)
' "$HOST_NAME" "$QARAX_NODE_HOST" "$QARAX_NODE_PORT"
}

echo "Registering host (${QARAX_NODE_HOST}:${QARAX_NODE_PORT})..."

resolved_name="$(resolve_existing_host_name || true)"
if [[ -n "$resolved_name" ]]; then
	HOST_NAME="$resolved_name"
	echo "Host already registered as '${HOST_NAME}', continuing with init." >&2
else

	add_attempts=10
	add_delay=2
	for add_attempt in $(seq 1 "$add_attempts"); do
		add_output=""
		if add_output=$(
			qarax host add \
				--name "$HOST_NAME" \
				--address "$QARAX_NODE_HOST" \
				--port "$QARAX_NODE_PORT" \
				--user root 2>&1
		); then
			[[ -n "$add_output" ]] && echo "$add_output"
			break
		fi

		# Uniqueness conflict: host already exists from a previous run — reuse it.
		if echo "$add_output" | grep -qi "already exists\|conflict\|unique"; then
			resolved_name="$(resolve_existing_host_name || true)"
			if [[ -n "$resolved_name" ]]; then
				HOST_NAME="$resolved_name"
				echo "Host already exists as '${HOST_NAME}', continuing with init." >&2
				break
			fi
			echo "Host already exists, but could not resolve its canonical name." >&2
		fi

		echo "Host add attempt ${add_attempt}/${add_attempts} failed: ${add_output}" >&2
		if [[ "$add_attempt" -lt "$add_attempts" ]]; then
			sleep "$add_delay"
		else
			echo "Host add failed after ${add_attempts} attempts for ${HOST_NAME}" >&2
			exit 1
		fi
	done
fi

attempts=10
delay_secs=2
for attempt in $(seq 1 "$attempts"); do
	if init_output=$(qarax host init "$HOST_NAME" 2>&1); then
		echo "$init_output"
		echo "Host initialized and set to UP (name: ${HOST_NAME})"
		exit 0
	fi

	echo "Host init attempt ${attempt}/${attempts} failed for ${HOST_NAME}: ${init_output}" >&2

	if [[ "$attempt" -lt "$attempts" ]]; then
		sleep "$delay_secs"
	fi
done

echo "Host init failed after ${attempts} attempts for ${HOST_NAME}" >&2
exit 1
