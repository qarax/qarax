#!/usr/bin/env bash
#
# Demo: run commands inside a VM with qarax vm exec
#
# Creates a VM with --guest-agent, starts it, and runs a command inside the
# guest. No boot source is needed: the stack's default kernel and initramfs
# boot qarax-init as PID 1, which serves guest exec over vsock.
#
# Prerequisites:
#   - qarax stack running (make run-local; started automatically if not up)
#   - jq
#
# Usage:
#   ./demos/vm-exec/run.sh [--server URL]
#   ./demos/vm-exec/run.sh --cleanup   # delete what an earlier run left behind
#   KEEP=1 ./demos/vm-exec/run.sh      # leave the VM running for inspection
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-vm-exec-"
VM_NAME="${PREFIX}vm"
EXEC_PAYLOAD='printf vm-exec && uname -s'
CLEANUP_ONLY=0

while [[ $# -gt 0 ]]; do
	case "$1" in
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
		echo ""
		echo "Options:"
		echo "  --server URL   qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --cleanup      Delete the demo's VM, then exit"
		echo ""
		echo "KEEP=1 leaves the VM running at exit."
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

command -v jq >/dev/null 2>&1 || die "jq is required"

ensure_stack
require_cli

if ((CLEANUP_ONLY)); then
	sweep vm "$PREFIX"
	ok "Removed the vm-exec demo's VM"
	exit 0
fi

banner "VM Guest Exec Demo"

step "Removing leftovers from earlier runs..."
sweep vm "$PREFIX"

cleanup() {
	echo
	if [[ "$KEEP" == 1 ]]; then
		info "KEEP=1: leaving VM '${VM_NAME}' in place (remove it with: $0 --cleanup)"
		return
	fi
	step "Cleaning up..."
	sweep vm "$PREFIX"
	info "Done."
}
trap cleanup EXIT

step "Creating a VM with the guest agent enabled..."
info "No boot source: the default kernel and initramfs boot qarax-init, which serves exec over vsock."
run qarax vm create --name "$VM_NAME" --vcpus 1 --memory 256MiB --guest-agent

step "Starting the VM..."
run qarax vm start "$VM_NAME"
run qarax vm get "$VM_NAME"

step "Waiting for the guest agent..."
wait_until 60 guest_agent_ready "$VM_NAME" ||
	die "The guest agent did not answer within 60s; see the guest console: qarax vm console ${VM_NAME}"
ok "Guest agent is up"

step "Running a command inside the guest..."
STDOUT=$(capture qarax vm exec "$VM_NAME" --timeout 15 -- /bin/sh -c "$EXEC_PAYLOAD")
echo "    ${STDOUT}"
[[ "$STDOUT" == "vm-execLinux" ]] || die "Unexpected guest output: '${STDOUT}' (expected 'vm-execLinux')"

banner "Demo Complete"
ok "Ran '/bin/sh -c \"${EXEC_PAYLOAD}\"' inside the guest through the guest agent"
