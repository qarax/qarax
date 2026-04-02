#!/usr/bin/env bash
#
# Demo: qarax lifecycle hooks
#
# Shows webhook-based lifecycle hooks firing on VM state transitions.
# Starts a tiny webhook receiver, creates a hook, runs a VM through its
# lifecycle, and displays the webhook payloads as they arrive in real-time.
#
# Prerequisites:
#   - qarax stack running (make run-local)
#   - qarax CLI on PATH
#   - jq installed
#
# Usage:
#   ./demos/hooks/run.sh
#   ./demos/hooks/run.sh --server http://localhost:8000
#   WEBHOOK_HOST=192.168.1.10 ./demos/hooks/run.sh  # if host.docker.internal doesn't work
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
WEBHOOK_PORT="${WEBHOOK_PORT:-9999}"
# How qarax (inside Docker) reaches the webhook receiver on the host.
# On Linux, containers reach the host via their Docker network gateway.
# On macOS/Windows Docker Desktop, host.docker.internal works.
_default_webhook_host() {
	if [[ "$(uname -s)" == "Linux" ]]; then
		local gw
		gw=$(docker network inspect e2e_default 2>/dev/null |
			python3 -c "import json,sys; cfg=json.load(sys.stdin)[0]['IPAM']['Config']; print(cfg[0]['Gateway'])" 2>/dev/null) ||
			gw=$(docker network inspect bridge 2>/dev/null |
				python3 -c "import json,sys; cfg=json.load(sys.stdin)[0]['IPAM']['Config']; print(cfg[0]['Gateway'])" 2>/dev/null) ||
			gw="172.17.0.1"
		echo "$gw"
	else
		echo "host.docker.internal"
	fi
}
WEBHOOK_HOST="${WEBHOOK_HOST:-$(_default_webhook_host)}"
HOOK_NAME="demo-hook-$$"
VM_NAME="hooks-demo-$$"
VCPUS=1
MEMORY_MIB=256
MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))
HOOK_ID=""
VM_ID=""

# Parse arguments
while [[ $# -gt 0 ]]; do
	case $1 in
	--server)
		SERVER="$2"
		shift 2
		;;
	--webhook-host)
		WEBHOOK_HOST="$2"
		shift 2
		;;
	--webhook-port)
		WEBHOOK_PORT="$2"
		shift 2
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo "  --server URL          qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --webhook-host HOST   How qarax reaches this machine (default: host.docker.internal)"
		echo "  --webhook-port PORT   Local port for webhook receiver (default: 9999)"
		exit 0
		;;
	*)
		echo "Unknown option: $1"
		exit 1
		;;
	esac
done

banner() {
	echo -e "\n${BOLD}${CYAN}══════════════════════════════════════════════════════════════${NC}"
	echo -e "${BOLD}${CYAN}  $1${NC}"
	echo -e "${BOLD}${CYAN}══════════════════════════════════════════════════════════════${NC}\n"
}
step() { echo -e "${GREEN}▸${NC} ${BOLD}$1${NC}"; }
info() { echo -e "  ${DIM}$1${NC}"; }
run() {
	echo -e "  ${DIM}\$ $*${NC}"
	"$@"
}

if [[ -z "$(find_qarax_bin)" ]]; then
	echo "qarax CLI not found — building..."
	cargo build -p cli
fi

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"
QARAX="$QARAX_BIN --server $SERVER"

ensure_stack "$SERVER"

cleanup() {
	echo
	step "Cleaning up..."
	if [[ -n "${RECEIVER_PID:-}" ]]; then
		kill "$RECEIVER_PID" 2>/dev/null || true
		wait "$RECEIVER_PID" 2>/dev/null || true
	fi
	if [[ -n "$VM_ID" ]]; then
		$QARAX vm delete "$VM_ID" 2>/dev/null || true
	fi
	if [[ -n "$HOOK_ID" ]]; then
		$QARAX hook delete "$HOOK_ID" 2>/dev/null || true
	fi
	info "Done."
}
trap cleanup EXIT

banner "Lifecycle Hooks Demo"

step "Starting webhook receiver on port $WEBHOOK_PORT..."

python3 -u -c "
import json, sys
from http.server import HTTPServer, BaseHTTPRequestHandler
from datetime import datetime

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get('Content-Length', 0))
        body = json.loads(self.rfile.read(length))
        sig = self.headers.get('X-Qarax-Signature', 'none')

        ts = datetime.now().strftime('%H:%M:%S.%f')[:-3]
        vm = body.get('vm_name', '?')
        prev = body.get('previous_status', '?')
        new = body.get('new_status', '?')
        tags = body.get('tags', [])
        tag_str = f'  tags={tags}' if tags else ''
        print(f'  \033[1;33m⚡ WEBHOOK\033[0m [{ts}] \033[1m{vm}\033[0m: {prev} → \033[1m{new}\033[0m{tag_str}', flush=True)

        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'ok')

    def log_message(self, *a): pass

print(f'  \033[2mListening on 0.0.0.0:${WEBHOOK_PORT}\033[0m', flush=True)
HTTPServer(('0.0.0.0', ${WEBHOOK_PORT}), Handler).serve_forever()
" &
RECEIVER_PID=$!
sleep 0.5

step "Creating lifecycle hook..."
echo
run $QARAX hook create \
	--name "$HOOK_NAME" \
	--url "http://${WEBHOOK_HOST}:${WEBHOOK_PORT}/webhook" \
	--scope global \
	--secret "demo-secret-key"
echo

HOOK_ID=$($QARAX hook list -o json | jq -r ".[] | select(.name == \"$HOOK_NAME\") | .id")
info "Hook ID: $HOOK_ID"

step "Hook registered:"
run $QARAX hook get "$HOOK_ID"
echo

banner "Running VM Lifecycle  (watch for ⚡ webhooks below)"

step "Creating VM '$VM_NAME'..."
run $QARAX vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "$MEMORY_BYTES"
echo

VM_ID=$($QARAX vm list -o json | jq -r ".[] | select(.name == \"$VM_NAME\") | .id")
info "VM ID: $VM_ID"
sleep 3 # let hook executor deliver

step "Starting VM..."
run $QARAX vm start "$VM_NAME"
info "Waiting for VM to start..."

# Poll until running or timeout
for i in $(seq 1 30); do
	STATUS=$($QARAX vm get "$VM_ID" -o json | jq -r '.status')
	if [[ "$STATUS" == "running" ]]; then
		break
	fi
	sleep 1
done
echo
sleep 3 # let hook executor deliver

step "Pausing VM..."
run $QARAX vm pause "$VM_NAME"
sleep 3

step "Resuming VM..."
run $QARAX vm resume "$VM_NAME"
sleep 3

step "Stopping VM..."
run $QARAX vm stop "$VM_NAME"
info "Waiting for VM to stop..."
for i in $(seq 1 20); do
	STATUS=$($QARAX vm get "$VM_ID" -o json | jq -r '.status')
	if [[ "$STATUS" == "shutdown" ]]; then
		break
	fi
	sleep 1
done
echo
sleep 3 # let hook executor deliver

step "Deleting VM..."
run $QARAX vm delete "$VM_NAME"
VM_ID="" # prevent double-delete in cleanup
sleep 3  # let hook executor deliver

banner "Hook Execution History"

step "All webhook deliveries for '$HOOK_NAME':"
echo
run $QARAX hook executions "$HOOK_ID"

echo
banner "Demo Complete"
