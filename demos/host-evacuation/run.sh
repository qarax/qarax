#!/usr/bin/env bash
#
# Demo: host maintenance / manual evacuation
#
# Shows a two-host live evacuation workflow end to end:
#   1. Make sure two hosts are UP
#   2. Create and start a small migration-compatible VM
#   3. Evacuate the VM's host
#   4. Show the host ends in maintenance and the VM is running on the other host
#   5. Create another VM and show scheduling avoids the maintenance host
#
# Prerequisites:
#   - local qarax stack (make run-local); started via hack/run-local.sh if not
#     running. The second node (qarax-node-2) is started and registered
#     automatically.
#   - jq
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-evac-"
VM="${PREFIX}vm"
FOLLOWUP_VM="${PREFIX}followup"

CLEANUP_ONLY=0

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --cleanup      Delete the demo VMs and return hosts in maintenance to UP, then exit
  --server URL   qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)
  --help, -h     Show this help

Environment:
  KEEP=1         Leave the VMs in place and the evacuated host in maintenance at exit
EOF
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--cleanup)
		CLEANUP_ONLY=1
		shift
		;;
	--server)
		SERVER="${2:?--server requires a URL}"
		shift 2
		;;
	--help | -h)
		usage
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

host_name() {
	qarax host get "$1" -o json | jq -r '.name'
}

# Return every host in maintenance to UP. The demo leaves the evacuated host
# in maintenance until exit, so an interrupted or KEEP=1 run leaves it there.
release_hosts() {
	local host
	for host in $(qarax host list -o json | jq -r '.[] | select(.status == "maintenance") | .name'); do
		run qarax host maintenance exit "$host"
	done
}

sweep_all() {
	sweep vm "$PREFIX"
	release_hosts
}

cleanup() {
	if [[ "$KEEP" == 1 ]]; then
		info "KEEP=1: leaving ${PREFIX}* VMs, and ${SOURCE_HOST:-the evacuated host} in maintenance. Undo with: $0 --cleanup"
		return
	fi
	step "Cleaning up"
	sweep_all
}

banner "Host Maintenance / Evacuation Demo"

command -v jq >/dev/null || die "jq is required"
ensure_stack
require_cli

step "Clearing leftovers from earlier runs"
sweep_all
if ((CLEANUP_ONLY)); then
	ok "Done"
	exit 0
fi

ensure_up_hosts 2
trap cleanup EXIT
run qarax host list

step "Creating and starting a migration-compatible VM"
info "The local stack's default boot (shared kernel + initramfs) supports live migration."
run qarax vm create --name "$VM" --vcpus 1 --memory 256MiB
run qarax vm start "$VM"
SOURCE_HOST_ID=$(qarax vm get "$VM" -o json | jq -r '.host_id')
SOURCE_HOST=$(host_name "$SOURCE_HOST_ID")
ok "${VM} is running on ${SOURCE_HOST}"

# Evacuation moves every VM on the host. Other demos leave VMs running, and
# some can't migrate (local-pool disks), so check before taking the host down.
others=$(qarax vm list -o json | jq -r --arg h "$SOURCE_HOST_ID" --arg vm "$VM" \
	'[.[] | select(.host_id == $h and .name != $vm) | .name] | join(", ")')
[[ -z "$others" ]] ||
	die "Evacuating ${SOURCE_HOST} would move every VM on it, and these aren't this demo's: ${others}\nRemove them (e.g. ./demos/<demo>/run.sh --cleanup) and re-run."

step "Evacuating ${SOURCE_HOST}"
info "Blocks until the evacuation job has migrated every VM off the host."
run qarax host evacuate "$SOURCE_HOST"

read -r status vm_host_id < <(qarax vm get "$VM" -o json | jq -r '"\(.status) \(.host_id)"')
[[ "$vm_host_id" != "$SOURCE_HOST_ID" ]] || die "${VM} is still on ${SOURCE_HOST}"
[[ "$status" == running ]] || die "${VM} is '${status}' after the migration, expected running"
ok "${VM} is running on $(host_name "$vm_host_id")"
host_status=$(qarax host get "$SOURCE_HOST" -o json | jq -r '.status')
[[ "$host_status" == maintenance ]] || die "${SOURCE_HOST} is '${host_status}' after evacuation, expected maintenance"
ok "${SOURCE_HOST} is in maintenance"
run qarax host list

step "Creating another VM to prove scheduling avoids the maintenance host"
run qarax vm create --name "$FOLLOWUP_VM" --vcpus 1 --memory 256MiB
followup_host_id=$(qarax vm get "$FOLLOWUP_VM" -o json | jq -r '.host_id')
[[ "$followup_host_id" != "$SOURCE_HOST_ID" ]] ||
	die "the scheduler placed ${FOLLOWUP_VM} on maintenance host ${SOURCE_HOST}"
ok "${FOLLOWUP_VM} was scheduled onto $(host_name "$followup_host_id"), not ${SOURCE_HOST}"

banner "Demo Complete"
ok "Evacuating ${SOURCE_HOST} live-migrated ${VM} away and left the host in maintenance"
ok "New VMs are scheduled elsewhere until the host leaves maintenance"
