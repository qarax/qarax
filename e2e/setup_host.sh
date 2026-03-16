#!/usr/bin/env bash
# Register the configured qarax-node host and initialize it.
# Required for VM scheduling (the control plane picks a host in UP state).
#
# In VM mode, pass the VM IP explicitly as QARAX_NODE_HOST or as the second arg.
# Usage: ./setup_host.sh [QARAX_URL] [QARAX_NODE_HOST] [QARAX_NODE_PORT] [HOST_NAME]

set -e

QARAX_URL="${1:-http://localhost:8000}"
QARAX_NODE_HOST="${2:-qarax-node}"
QARAX_NODE_PORT="${3:-50051}"
HOST_NAME="${4:-e2e-node}"

MUSL_TARGET="x86_64-unknown-linux-musl"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

find_qarax_bin() {
	if command -v qarax &>/dev/null; then
		echo "qarax"
	elif [[ -x "$REPO_ROOT/target/$MUSL_TARGET/release/qarax" ]]; then
		echo "$REPO_ROOT/target/$MUSL_TARGET/release/qarax"
	elif [[ -x "$REPO_ROOT/target/$MUSL_TARGET/debug/qarax" ]]; then
		echo "$REPO_ROOT/target/$MUSL_TARGET/debug/qarax"
	else
		echo ""
	fi
}

QARAX_BIN="$(find_qarax_bin)"
if [[ -z "$QARAX_BIN" ]]; then
	echo "ERROR: qarax CLI binary not found. Build it with: cargo build -p cli" >&2
	exit 1
fi

QARAX="$QARAX_BIN --server $QARAX_URL"

echo "Registering host (${QARAX_NODE_HOST}:${QARAX_NODE_PORT})..."

# Attempt registration; ignore errors since the host may already exist
# (the address column has a UNIQUE constraint).
$QARAX host add \
	--name "$HOST_NAME" \
	--address "$QARAX_NODE_HOST" \
	--port "$QARAX_NODE_PORT" \
	--user root \
	--password "" 2>/dev/null || true

$QARAX host init "$HOST_NAME"

echo "Host initialized and set to UP (name: ${HOST_NAME})"
