#!/usr/bin/env bash
#
# Demo: qarax SSE event stream
#
# Demonstrates the GET /events endpoint which streams VM status-change events
# via Server-Sent Events (SSE) in real-time. Opens two SSE subscriptions:
#   1. Full stream  — captures every vm.status_changed event
#   2. Filtered     — captures only transitions TO "running"
#
# Then runs a VM through its lifecycle (create → start → stop → delete) and
# prints each received event, showing status transitions as they happen.
#
# Prerequisites:
#   - qarax stack running (make run-local)
#   - qarax CLI on PATH or built (cargo build -p cli)
#   - curl and jq installed
#
# Usage:
#   ./demos/sse-events/run.sh
#   ./demos/sse-events/run.sh --server http://localhost:8000
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
VM_NAME="sse-demo-$$"
VCPUS=1
MEMORY_MIB=256
MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))
VM_ID=""
SSE_ALL_PID=""
SSE_FILTERED_PID=""
SSE_ALL_LOG=""
SSE_FILTERED_LOG=""

# Parse arguments
while [[ $# -gt 0 ]]; do
	case $1 in
	--server)
		SERVER="$2"
		shift 2
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo "  --server URL   qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
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

# Find the qarax binary, building if necessary
if [[ -z "$(find_qarax_bin)" ]]; then
	echo "qarax CLI not found — building..."
	cargo build -p cli
fi
QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"
QARAX="$QARAX_BIN --server $SERVER"

ensure_stack "$SERVER"

# Pretty-print an SSE event line───────
# Reads from a log file and renders each data: line as a formatted transition.
print_events() {
	local label="$1"
	local log_file="$2"
	local count=0

	while IFS= read -r line; do
		if [[ "$line" == data:* ]]; then
			local payload="${line#data: }"
			local vm_name prev_status new_status ts
			vm_name=$(echo "$payload" | jq -r '.vm_name // "?"')
			prev_status=$(echo "$payload" | jq -r '.previous_status // "?"')
			new_status=$(echo "$payload" | jq -r '.new_status // "?"')
			ts=$(echo "$payload" | jq -r '.timestamp // "?"' | sed 's/T/ /; s/\..*//')
			echo -e "  ${YELLOW}${label}${NC} [${DIM}${ts}${NC}] ${BOLD}${vm_name}${NC}: ${DIM}${prev_status}${NC} → ${BOLD}${new_status}${NC}"
			count=$((count + 1))
		fi
	done <"$log_file"
	echo -e "  ${DIM}(${count} event(s) received)${NC}"
}

cleanup() {
	echo
	step "Cleaning up..."
	if [[ -n "${SSE_ALL_PID:-}" ]]; then
		kill "$SSE_ALL_PID" 2>/dev/null || true
		wait "$SSE_ALL_PID" 2>/dev/null || true
	fi
	if [[ -n "${SSE_FILTERED_PID:-}" ]]; then
		kill "$SSE_FILTERED_PID" 2>/dev/null || true
		wait "$SSE_FILTERED_PID" 2>/dev/null || true
	fi
	[[ -n "${SSE_ALL_LOG:-}" ]] && rm -f "$SSE_ALL_LOG"
	[[ -n "${SSE_FILTERED_LOG:-}" ]] && rm -f "$SSE_FILTERED_LOG"
	if [[ -n "${VM_ID:-}" ]]; then
		$QARAX vm delete "$VM_ID" 2>/dev/null || true
	fi
	info "Done."
}
trap cleanup EXIT

# Verify stack is reachable─────────
banner "SSE Event Stream Demo"

step "Verifying qarax stack at $SERVER..."
if ! curl -sf "$SERVER/hosts" >/dev/null; then
	die "Cannot reach qarax at $SERVER — run 'make run-local' first"
fi
info "Stack is up."

# Open SSE subscriptions
banner "Opening SSE Subscriptions"

SSE_ALL_LOG=$(mktemp /tmp/sse-all-XXXXXX.log)
SSE_FILTERED_LOG=$(mktemp /tmp/sse-filtered-XXXXXX.log)

step "Opening unfiltered SSE stream  →  GET ${SERVER}/events"
curl -sN --no-buffer "${SERVER}/events" >>"$SSE_ALL_LOG" 2>&1 &
SSE_ALL_PID=$!
info "Background PID: $SSE_ALL_PID  |  log: $SSE_ALL_LOG"

step "Opening filtered SSE stream    →  GET ${SERVER}/events?status=running"
curl -sN --no-buffer "${SERVER}/events?status=running" >>"$SSE_FILTERED_LOG" 2>&1 &
SSE_FILTERED_PID=$!
info "Background PID: $SSE_FILTERED_PID  |  log: $SSE_FILTERED_LOG"

# Give curl connections time to establish
sleep 1

# VM lifecycle
banner "Running VM Lifecycle"

step "Creating VM '$VM_NAME' (${VCPUS} vCPU, ${MEMORY_MIB} MiB)..."
run $QARAX vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "$MEMORY_BYTES"
echo

VM_ID=$($QARAX vm list -o json |
	jq -r ".[] | select(.name == \"$VM_NAME\") | .id")
info "VM ID: $VM_ID"

step "Starting VM..."
run $QARAX vm start "$VM_NAME"
info "Waiting for VM to reach 'running'..."
for i in $(seq 1 30); do
	STATUS=$($QARAX vm get "$VM_ID" -o json | jq -r '.status')
	if [[ "$STATUS" == "running" ]]; then break; fi
	sleep 1
done
echo

step "Stopping VM..."
run $QARAX vm stop "$VM_NAME"
info "Waiting for VM to reach 'shutdown'..."
for i in $(seq 1 30); do
	STATUS=$($QARAX vm get "$VM_ID" -o json | jq -r '.status')
	if [[ "$STATUS" == "shutdown" ]]; then break; fi
	sleep 1
done
echo

step "Deleting VM..."
run $QARAX vm delete "$VM_NAME"
VM_ID="" # prevent double-delete in cleanup
echo

# Brief pause to allow any final events to be written to the log files
sleep 1

# Display captured events
banner "Captured SSE Events"

step "All events (unfiltered stream):"
echo
print_events "ALL   " "$SSE_ALL_LOG"
echo

step "Filtered stream  (?status=running  — only transitions TO 'running'):"
echo
print_events "RUNNING" "$SSE_FILTERED_LOG"
echo

# Raw log sample
banner "Raw SSE Wire Format (first 20 lines)"

info "The SSE protocol sends events as:"
info "  event: <event-type>"
info "  id: <vm-uuid>"
info "  data: <json-payload>"
echo
head -20 "$SSE_ALL_LOG" 2>/dev/null || true

banner "Demo Complete"

echo -e "  Try it yourself:"
echo -e "  ${DIM}\$ curl -N '${SERVER}/events'${NC}"
echo -e "  ${DIM}\$ curl -N '${SERVER}/events?status=running'${NC}"
echo -e "  ${DIM}\$ curl -N '${SERVER}/events?vm_id=<uuid>'${NC}"
echo -e "  ${DIM}\$ curl -N '${SERVER}/events?tag=<tag>'${NC}"
echo
