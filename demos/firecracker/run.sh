#!/usr/bin/env bash
#
# Demo: Firecracker backend
#
# Runs a VM under Firecracker (--hypervisor firecracker) through its
# lifecycle: create, start, pause, resume, stop. Shows the VM's hypervisor
# and the guest's boot output from Firecracker's serial console.
#
# Prerequisites:
#   - qarax stack running (make run-local; started automatically if not up)
#   - Firecracker on qarax-node (the local qarax-node image ships it)
#   - jq
#
# Usage:
#   ./demos/firecracker/run.sh [--server URL] [--vcpus N] [--memory MiB]
#   ./demos/firecracker/run.sh --cleanup   # delete what an earlier run left behind
#   KEEP=1 ./demos/firecracker/run.sh      # leave the VM in place
#   SKIP_BUILD=1 ./demos/firecracker/run.sh  # use an existing qarax CLI build
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-firecracker-"
VM_NAME="${PREFIX}vm"
VCPUS=1
MEMORY_MIB=128
# qarax-node's --runtime-dir default, where Firecracker's log (with the
# guest's serial console) lives.
NODE_RUNTIME_DIR="/var/lib/qarax/vms"
CLEANUP_ONLY=0

while [[ $# -gt 0 ]]; do
	case "$1" in
	--server)
		SERVER="$2"
		shift 2
		;;
	--vcpus)
		VCPUS="$2"
		shift 2
		;;
	--memory)
		MEMORY_MIB="$2"
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
		echo "  --server URL      qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --vcpus N         vCPU count (default: 1)"
		echo "  --memory MiB      memory in MiB (default: 128)"
		echo "  --cleanup         Delete the demo's VM, then exit"
		echo ""
		echo "KEEP=1 leaves the VM in place at exit."
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

expect_status() {
	local status
	status=$(vm_status "$VM_NAME")
	[[ "$status" == "$1" ]] || die "VM '${VM_NAME}' is '${status}', expected '$1'"
	ok "status: ${status}"
}

# `qarax vm console` reads Cloud Hypervisor's console log. Firecracker writes
# the guest's serial console into its own log in the node's runtime dir, so
# read that from the node container (local stack only).
show_guest_console() {
	local address service log
	address=$(qarax host get "$(jq -r '.host_id' <<<"$VM_JSON")" -o json | jq -r '.address')
	if ! service=$(node_service_for_address "$address"); then
		warn "Host ${address} is not a local compose node; skipping the guest console."
		return 0
	fi
	log="${NODE_RUNTIME_DIR}/$(jq -r '.id' <<<"$VM_JSON").fc.log"
	info "${service}:${log}"
	wait_until 30 node_exec "$service" grep -q 'qarax-init: starting' "$log" ||
		die "The guest did not reach qarax-init within 30s; see ${log} in the ${service} container"
	node_exec "$service" sh -c 'grep -m1 "Linux version" "$1"; grep -m1 "qarax-init: starting" "$1"' _ "$log" |
		tr -d '\r' | sed 's/^/    /'
}

command -v jq >/dev/null 2>&1 || die "jq is required"

ensure_stack
require_cli

if ((CLEANUP_ONLY)); then
	sweep vm "$PREFIX"
	ok "Removed the Firecracker demo's VM"
	exit 0
fi

banner "Firecracker Demo"

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

step "Checking for a Firecracker-capable host..."
qarax host list -o json |
	jq -r '.[] | select(.status == "up") | "  \(.name): Firecracker \(.firecracker_version // "not installed")"'
qarax host list -o json | jq -e 'any(.[]; .status == "up" and .firecracker_version != null)' >/dev/null ||
	die "No UP host reports a Firecracker version; qarax-node needs /usr/local/bin/firecracker and /dev/kvm"

step "Creating a Firecracker VM..."
run qarax vm create \
	--name "$VM_NAME" \
	--hypervisor firecracker \
	--vcpus "$VCPUS" \
	--memory "${MEMORY_MIB}MiB"

step "Starting the VM..."
run qarax vm start "$VM_NAME"
run qarax vm get "$VM_NAME"
VM_JSON=$(qarax vm get "$VM_NAME" -o json)
[[ "$(jq -r '.hypervisor' <<<"$VM_JSON")" == "firecracker" ]] ||
	die "VM '${VM_NAME}' is not a Firecracker VM"

step "Guest boot output from Firecracker's serial console..."
show_guest_console

step "Pausing the VM..."
run qarax vm pause "$VM_NAME"
expect_status paused

step "Resuming the VM..."
run qarax vm resume "$VM_NAME"
expect_status running

step "Stopping the VM..."
run qarax vm stop "$VM_NAME"
expect_status shutdown

banner "Demo Complete"
ok "Created, started, paused, resumed and stopped a Firecracker VM"
