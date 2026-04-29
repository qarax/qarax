#!/usr/bin/env bash
#
# Demo: host maintenance / manual evacuation
#
# Shows a two-host live evacuation workflow end to end:
#   1. Verify two hosts are UP
#   2. Create and start a small migration-compatible VM
#   3. Identify the VM's source host
#   4. Evacuate that host
#   5. Show the host ends in maintenance and the VM moves away
#   6. Create another VM and show scheduling avoids the maintenance host
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

cd "$REPO_ROOT"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
SUFFIX="$$"
VM_NAME="demo-evacuate-vm-${SUFFIX}"
FOLLOWUP_VM_NAME="demo-evacuate-post-${SUFFIX}"
VCPUS=1
MEMORY_MIB=256
MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))

SOURCE_HOST_ID=""
SOURCE_HOST_NAME=""
DEST_HOST_ID=""
DEST_HOST_NAME=""
FOLLOWUP_HOST_ID=""
FOLLOWUP_HOST_NAME=""
PRIMARY_VM_CREATED=0
FOLLOWUP_VM_CREATED=0

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

usage() {
	echo "Usage: $0 [OPTIONS]"
	echo ""
	echo "Options:"
	echo "  --server URL   qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
	echo "  --help, -h     Show this help"
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--server)
		SERVER="$2"
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

if [[ -z "$(find_qarax_bin)" ]]; then
	echo "qarax CLI not found — building..."
	cargo build -p cli
fi

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"

qarax() {
	"$QARAX_BIN" --server "$SERVER" "$@"
}

ensure_two_host_stack() {
	if curl -sf --max-time 3 "${SERVER}/" >/dev/null 2>&1; then
		return 0
	fi

	die "qarax server not reachable at ${SERVER}\nStart a live two-node stack first, for example:\n  cd e2e\n  KEEP=1 ./run_e2e_tests.sh test_live_migration.py::test_host_evacuation_marks_maintenance_and_avoids_rescheduling"
}

host_name_for_id() {
	qarax host get "$1" -o json | jq -r '.name'
}

wait_for_vm_status() {
	local vm="$1"
	local target="$2"
	local timeout="${3:-90}"
	local elapsed=0

	while [[ $elapsed -lt $timeout ]]; do
		local status
		status="$(qarax vm get "$vm" -o json | jq -r '.status')"
		if [[ "$status" == "$target" ]]; then
			info "${vm}: ${status}"
			return 0
		fi
		sleep 2
		elapsed=$((elapsed + 2))
	done

	die "Timed out waiting for VM ${vm} to reach status '${target}'"
}

wait_for_host_status() {
	local host="$1"
	local target="$2"
	local timeout="${3:-60}"
	local elapsed=0

	while [[ $elapsed -lt $timeout ]]; do
		local status
		status="$(qarax host get "$host" -o json | jq -r '.status')"
		if [[ "$status" == "$target" ]]; then
			info "${host}: ${status}"
			return 0
		fi
		sleep 2
		elapsed=$((elapsed + 2))
	done

	die "Timed out waiting for host ${host} to reach status '${target}'"
}

wait_for_vm_host_change() {
	local vm="$1"
	local original_host_id="$2"
	local timeout="${3:-90}"
	local elapsed=0

	while [[ $elapsed -lt $timeout ]]; do
		local current_host_id
		current_host_id="$(qarax vm get "$vm" -o json | jq -r '.host_id // empty')"
		if [[ -n "$current_host_id" && "$current_host_id" != "$original_host_id" ]]; then
			printf '%s\n' "$current_host_id"
			return 0
		fi
		sleep 2
		elapsed=$((elapsed + 2))
	done

	die "Timed out waiting for VM ${vm} to move off host ${original_host_id}"
}

cleanup() {
	echo
	step "Cleaning up..."

	if [[ "$FOLLOWUP_VM_CREATED" -eq 1 ]]; then
		qarax vm stop "$FOLLOWUP_VM_NAME" 2>/dev/null || true
		qarax vm delete "$FOLLOWUP_VM_NAME" 2>/dev/null || true
	fi

	if [[ "$PRIMARY_VM_CREATED" -eq 1 ]]; then
		qarax vm stop "$VM_NAME" 2>/dev/null || true
		qarax vm delete "$VM_NAME" 2>/dev/null || true
	fi

	if [[ -n "$SOURCE_HOST_NAME" ]]; then
		current_status="$(qarax host get "$SOURCE_HOST_NAME" -o json 2>/dev/null | jq -r '.status // empty' 2>/dev/null || true)"
		if [[ "$current_status" == "maintenance" ]]; then
			qarax host maintenance exit "$SOURCE_HOST_NAME" 2>/dev/null || true
		fi
	fi

	info "Done."
}
trap cleanup EXIT

banner "Host Maintenance / Evacuation Demo"

step "Preflight checks"
command -v jq >/dev/null || die "jq is required"
ensure_two_host_stack

host_json="$(qarax host list -o json 2>&1)" || {
	if grep -qi "missing field" <<<"$host_json"; then
		die "CLI/server schema mismatch detected. Rebuild the local stack with: REBUILD=1 ./hack/run-local.sh"
	fi
	die "Failed to list hosts: $host_json"
}

up_hosts="$(jq -r '.[] | select(.status == "up") | [.name, .address] | @tsv' <<<"$host_json")"
up_host_count="$(jq -r '[.[] | select(.status == "up")] | length' <<<"$host_json")"
[[ "$up_host_count" -ge 2 ]] || die "This demo requires two UP hosts. Start the two-node e2e stack first."

info "UP hosts:"
while IFS=$'\t' read -r host_name host_address; do
	[[ -n "${host_name:-}" ]] || continue
	info "- ${host_name} (${host_address})"
done <<<"$up_hosts"
echo
run qarax host list
echo

step "Creating and starting a migration-compatible VM"
info "This demo uses the local two-node test stack's default boot configuration, which already supports live migration."
run qarax vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "$MEMORY_BYTES"
PRIMARY_VM_CREATED=1
run qarax vm start "$VM_NAME"
wait_for_vm_status "$VM_NAME" running 120

SOURCE_HOST_ID="$(qarax vm get "$VM_NAME" -o json | jq -r '.host_id // empty')"
[[ -n "$SOURCE_HOST_ID" ]] || die "VM ${VM_NAME} does not have an assigned source host"
SOURCE_HOST_NAME="$(host_name_for_id "$SOURCE_HOST_ID")"
info "${VM_NAME} initially landed on ${SOURCE_HOST_NAME}"
echo
run qarax vm get "$VM_NAME"
echo

step "Evacuating source host ${SOURCE_HOST_NAME}"
run qarax host evacuate "$SOURCE_HOST_NAME"
wait_for_host_status "$SOURCE_HOST_NAME" maintenance 60
wait_for_vm_status "$VM_NAME" running 120
DEST_HOST_ID="$(wait_for_vm_host_change "$VM_NAME" "$SOURCE_HOST_ID" 120)"
DEST_HOST_NAME="$(host_name_for_id "$DEST_HOST_ID")"
[[ "$DEST_HOST_ID" != "$SOURCE_HOST_ID" ]] || die "VM ${VM_NAME} did not move off ${SOURCE_HOST_NAME}"
info "${VM_NAME} moved from ${SOURCE_HOST_NAME} to ${DEST_HOST_NAME}"
echo
run qarax host get "$SOURCE_HOST_NAME"
echo
run qarax vm get "$VM_NAME"
echo

step "Creating another VM to prove scheduling avoids the maintenance host"
run qarax vm create --name "$FOLLOWUP_VM_NAME" --vcpus "$VCPUS" --memory "$MEMORY_BYTES"
FOLLOWUP_VM_CREATED=1
FOLLOWUP_HOST_ID="$(qarax vm get "$FOLLOWUP_VM_NAME" -o json | jq -r '.host_id // empty')"
[[ -n "$FOLLOWUP_HOST_ID" ]] || die "VM ${FOLLOWUP_VM_NAME} was not scheduled onto a host"
FOLLOWUP_HOST_NAME="$(host_name_for_id "$FOLLOWUP_HOST_ID")"
[[ "$FOLLOWUP_HOST_ID" != "$SOURCE_HOST_ID" ]] || die "Maintenance host ${SOURCE_HOST_NAME} was incorrectly selected for ${FOLLOWUP_VM_NAME}"
run qarax vm start "$FOLLOWUP_VM_NAME"
wait_for_vm_status "$FOLLOWUP_VM_NAME" running 120
info "${FOLLOWUP_VM_NAME} landed on ${FOLLOWUP_HOST_NAME}, not on maintenance host ${SOURCE_HOST_NAME}"
echo
run qarax vm get "$FOLLOWUP_VM_NAME"

echo
banner "Demo Complete"
info "Host ${SOURCE_HOST_NAME} entered maintenance after evacuation."
info "VM ${VM_NAME} live-migrated to ${DEST_HOST_NAME}."
info "New VM ${FOLLOWUP_VM_NAME} avoided the maintenance host."
