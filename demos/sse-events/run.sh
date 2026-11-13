#!/usr/bin/env bash
#
# Demo: qarax SSE event stream
#
# GET /events streams VM status changes as Server-Sent Events. The demo opens
# two subscriptions before touching the VM:
#   1. the full stream: every vm.status_changed event
#   2. a filtered stream (?status=running): only transitions to "running"
# then starts, pauses, resumes and stops a VM, printing each event as it
# arrives and checking that both streams delivered exactly the transitions
# they should.
#
# Prerequisites:
#   - qarax stack running (make run-local; started automatically if not up)
#   - curl and jq
#
# Usage:
#   ./demos/sse-events/run.sh [--server URL]
#   ./demos/sse-events/run.sh --cleanup   # delete what an earlier run left behind
#   KEEP=1 ./demos/sse-events/run.sh      # leave the VM in place
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-sse-"
VM_NAME="${PREFIX}vm"
CLEANUP_ONLY=0

while [[ $# -gt 0 ]]; do
	case $1 in
	--server)
		SERVER="$2"
		shift 2
		;;
	--cleanup)
		CLEANUP_ONLY=1
		shift
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo "  --server URL   qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --cleanup      Delete the demo's VM, then exit"
		echo ""
		echo "KEEP=1 leaves the VM in place at exit."
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

# open_stream NAME QUERY: subscribe to /events in the background and write the
# event stream to $WORK/NAME. The server subscribes the client before it sends
# the 200, so every later event reaches it. The headers go to a file of their
# own (-D), which curl writes as they arrive; with -i they'd sit in the
# buffered stdout until the first event. curl runs directly (not through
# api_curl) so $! is its PID.
open_stream() {
	local log="${WORK}/$1" url="${SERVER}/events$2"
	echo -e "  ${DIM}\$ curl -N -H \"Authorization: Bearer \$QARAX_TOKEN\" '${url}'${NC}"
	curl -sN -D "${log}.headers" -H "Authorization: Bearer ${QARAX_TOKEN}" "$url" >"$log" &
	STREAM_PIDS+=("$!")
	wait_until 5 grep -q '^HTTP/[0-9.]* 200' "${log}.headers" ||
		die "GET ${url} did not open: $(cat "${log}.headers" 2>/dev/null)"
}

# transitions NAME: the demo VM's transitions received on stream NAME so far,
# one "previous→new<TAB>time" per line. A data line still being written
# fails fromjson? and is read whole on the next call.
transitions() {
	sed -n 's/^data: //p' "${WORK}/$1" |
		jq -rR --arg vm "$VM_NAME" \
			'fromjson? | select(.vm_name == $vm) | "\(.previous_status)→\(.new_status)\t\(.timestamp[11:23])"'
}

# stream_has NAME TRANSITION...: stream NAME holds exactly these transitions.
stream_has() {
	local name="$1"
	shift
	[[ "$(transitions "$name" | cut -f1)" == "$(printf '%s\n' "$@")" ]]
}

streams_caught_up() {
	stream_has all "${ALL_WANT[@]}" && stream_has running "${RUNNING_WANT[@]}"
}

# print_events NAME LABEL SKIP: print the events on stream NAME after the first SKIP.
print_events() {
	local transition ts
	while IFS=$'\t' read -r transition ts; do
		echo -e "  ${YELLOW}$2${NC} [${DIM}${ts}${NC}] ${BOLD}${VM_NAME}${NC}: ${BOLD}${transition}${NC}"
	done < <(transitions "$1" | tail -n "+$(($3 + 1))")
}

# expect_events TRANSITION...: the VM's next transitions. All of them must
# arrive on the full stream, and those to "running" on the filtered stream
# too. Waits for both streams to catch up, then prints what each received.
ALL_WANT=()
RUNNING_WANT=()
expect_events() {
	local all_seen=${#ALL_WANT[@]} running_seen=${#RUNNING_WANT[@]} transition
	for transition in "$@"; do
		ALL_WANT+=("$transition")
		if [[ "$transition" == *→running ]]; then
			RUNNING_WANT+=("$transition")
		fi
	done
	if ! wait_until 10 streams_caught_up; then
		warn "full stream:     expected ${ALL_WANT[*]}, got $(transitions all | cut -f1 | paste -sd' ')"
		warn "?status=running: expected ${RUNNING_WANT[*]}, got $(transitions running | cut -f1 | paste -sd' ')"
		die "The SSE streams did not deliver the expected events"
	fi
	print_events all "ALL    " "$all_seen"
	print_events running "RUNNING" "$running_seen"
}

command -v jq >/dev/null 2>&1 || die "jq is required"

ensure_stack
require_cli

if ((CLEANUP_ONLY)); then
	sweep vm "$PREFIX"
	ok "Removed the SSE demo's VM"
	exit 0
fi

banner "SSE Event Stream Demo"

step "Removing leftovers from earlier runs..."
sweep vm "$PREFIX"

WORK=$(mktemp -d)
STREAM_PIDS=()

cleanup() {
	echo
	if ((${#STREAM_PIDS[@]})); then
		# A stream has ended on its own if the server went away.
		kill "${STREAM_PIDS[@]}" 2>/dev/null || true
	fi
	rm -rf "$WORK"
	if [[ "$KEEP" == 1 ]]; then
		info "KEEP=1: leaving VM '${VM_NAME}' in place (remove it with: $0 --cleanup)"
		return
	fi
	step "Cleaning up..."
	sweep vm "$PREFIX"
	info "Done."
}
trap cleanup EXIT

banner "Opening SSE Subscriptions"

step "Full stream: every vm.status_changed event"
open_stream all ""
step "Filtered stream: only transitions to 'running'"
open_stream running "?status=running"

banner "Running VM Lifecycle"

step "Creating VM '${VM_NAME}' (creation is not a status change, so no event)..."
run qarax vm create --name "$VM_NAME" --vcpus 1 --memory 256MiB

step "Starting VM..."
run qarax vm start "$VM_NAME"
expect_events "created→pending" "pending→running"

step "Pausing VM..."
run qarax vm pause "$VM_NAME"
expect_events "running→paused"

step "Resuming VM..."
run qarax vm resume "$VM_NAME"
expect_events "paused→running"

step "Stopping VM..."
run qarax vm stop "$VM_NAME"
expect_events "running→shutdown"

banner "Raw SSE Wire Format"

info "The first event as the full stream received it:"
awk '/^event:/ { p = 1 } p { print "    " $0 } p && /^$/ { exit }' "${WORK}/all"

banner "Demo Complete"

ok "The full stream delivered all ${#ALL_WANT[@]} transitions; ?status=running delivered only its ${#RUNNING_WANT[@]}"
info "Deleting a VM emits no SSE event (lifecycle hooks do get a 'deleted' delivery)."
echo
echo -e "  Try it yourself:"
echo -e "  ${DIM}\$ curl -N -H \"Authorization: Bearer \$QARAX_TOKEN\" '${SERVER}/events'${NC}"
echo -e "  ${DIM}\$ curl -N -H \"Authorization: Bearer \$QARAX_TOKEN\" '${SERVER}/events?status=running'${NC}"
echo -e "  ${DIM}\$ curl -N -H \"Authorization: Bearer \$QARAX_TOKEN\" '${SERVER}/events?vm_id=<uuid>'${NC}"
echo -e "  ${DIM}\$ curl -N -H \"Authorization: Bearer \$QARAX_TOKEN\" '${SERVER}/events?tag=<tag>'${NC}"
