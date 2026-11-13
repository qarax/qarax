#!/usr/bin/env bash
#
# Demo: VM commit: convert an OCI image-backed VM to a standalone raw disk VM
#
#   1. Create a Local storage pool for the committed raw disk
#   2. Create a VM from an OCI image (--image-ref); qarax imports it into the
#      shared OverlayBD pool
#   3. Run `vm commit` to copy the OverlayBD block device to a raw disk
#   4. Verify image_ref is cleared and the committed disk exists
#   5. Boot the committed VM
#
# The VM and the pool are left in place so you can explore them. A re-run
# replaces the VM; --cleanup deletes the VM, the committed disk and the pool.
#
# Prerequisites:
#   - qarax stack running (make run-local), or let the script start it.
#     make run-local registers the host and creates overlaybd-pool.
#   - cargo (to build the qarax CLI) or a qarax CLI on PATH
#   - jq
#
# Usage:
#   ./demos/vm-commit/run.sh
#   ./demos/vm-commit/run.sh --cleanup
#   QARAX_SERVER=http://localhost:8000 ./demos/vm-commit/run.sh
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-commit-"
VM_NAME="${PREFIX}vm"
LOCAL_POOL_NAME="${PREFIX}local"
LOCAL_POOL_PATH="/var/lib/qarax/storage/${LOCAL_POOL_NAME}" # on the node's volume
# Shared OverlayBD pool from make run-local. qarax picks the first OverlayBD
# pool by name for --image-ref VMs, so the demo must not add one of its own.
OBD_POOL_NAME="overlaybd-pool"
# An OverlayBD pool an older copy of this demo created (see sweep_demo).
LEGACY_OBD_POOL_NAME="${PREFIX}obd"
IMAGE_REF="public.ecr.aws/docker/library/busybox:latest"
COMMIT_SIZE=$((1024 * 1024 * 1024))
VCPUS=1
MEMORY_MIB=256

case "${1:-}" in
"" | --cleanup) ;;
--help | -h)
	echo "Usage: $0 [--cleanup]"
	echo "  --cleanup   Delete the VM, the committed disk and the local pool, then exit"
	echo "Environment: QARAX_SERVER, QARAX_TOKEN"
	exit 0
	;;
*) die "Unknown option: $1 (see --help)" ;;
esac

# delete_pool_objects POOL: delete every storage object in POOL.
delete_pool_objects() {
	local id
	for id in $(qarax storage-object list --pool "$1" -o json | jq -r '.[].id'); do
		qarax storage-object delete "$id" >/dev/null
	done
}

# delete_image_vm VM: delete an --image-ref VM and the OverlayBD image object
# qarax created for it (overlaybd-<vm id>), which VM deletion leaves behind.
# A committed VM no longer has that object.
delete_image_vm() {
	local id
	id="$(qarax vm get "$1" -o json | jq -r '.id')"
	qarax vm delete "$id" >/dev/null
	if exists storage-object "overlaybd-${id}"; then
		qarax storage-object delete "overlaybd-${id}" >/dev/null
	fi
}

# Delete what a previous run left, except the local pool.
sweep_demo() {
	if exists vm "$VM_NAME"; then
		delete_image_vm "$VM_NAME"
	fi
	if exists storage-pool "$LOCAL_POOL_NAME"; then
		delete_pool_objects "$LOCAL_POOL_NAME"
	fi
	# An older copy of this demo created its own OverlayBD pool, which stays
	# on stacks that ran it. Being first by name, it would serve every other
	# demo's --image-ref VMs.
	if exists storage-pool "$LEGACY_OBD_POOL_NAME"; then
		delete_pool_objects "$LEGACY_OBD_POOL_NAME"
		run qarax storage-pool delete "$LEGACY_OBD_POOL_NAME"
	fi
}

if [[ "${1:-}" == "--cleanup" ]]; then
	if ! server_reachable; then
		info "qarax server not reachable at ${SERVER}: nothing to clean up."
		exit 0
	fi
	require_auth
	require_cli
	step "Deleting vm-commit demo resources (${OBD_POOL_NAME} stays)"
	sweep_demo
	if exists storage-pool "$LOCAL_POOL_NAME"; then
		run qarax storage-pool delete "$LOCAL_POOL_NAME"
	fi
	ok "Done"
	exit 0
fi

ensure_stack
require_cli

banner "VM Commit Demo"
info "Server:          ${SERVER}"
info "VM:              ${VM_NAME}"
info "Image ref:       ${IMAGE_REF}"
info "OverlayBD pool:  ${OBD_POOL_NAME}"
info "Local pool:      ${LOCAL_POOL_NAME}"

exists storage-pool "$OBD_POOL_NAME" ||
	die "Storage pool '${OBD_POOL_NAME}' not found. Run 'make run-local' (it creates it)."
# The VM belongs to this demo: replace the one from a previous run.
sweep_demo

banner "Creating an OCI-backed VM"

step "Create '${VM_NAME}' from ${IMAGE_REF} (the CLI waits for the import)"
run qarax vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "${MEMORY_MIB}MiB" \
	--image-ref "$IMAGE_REF"
run qarax vm get "$VM_NAME"

VM_JSON="$(qarax vm get "$VM_NAME" -o json)"
VM_ID="$(jq -r '.id' <<<"$VM_JSON")"
IMAGE_REF_BEFORE="$(jq -r '.image_ref // empty' <<<"$VM_JSON")"
[[ -n "$IMAGE_REF_BEFORE" ]] || die "${VM_NAME} has no image_ref after creation"
ok "image_ref = ${IMAGE_REF_BEFORE}: the VM boots from the OCI image"

# A local pool lives on one host's disk; put it where the scheduler placed the VM.
HOST_NAME="$(qarax host get "$(jq -r '.host_id' <<<"$VM_JSON")" -o json | jq -r '.name')"
step "Local storage pool '${LOCAL_POOL_NAME}' for the committed raw disk"
if exists storage-pool "$LOCAL_POOL_NAME"; then
	ok "${LOCAL_POOL_NAME} exists, reusing it"
	run qarax storage-pool attach-host "$LOCAL_POOL_NAME" "$HOST_NAME"
else
	run qarax storage-pool create --name "$LOCAL_POOL_NAME" --pool-type local \
		--path "$LOCAL_POOL_PATH" --host "$HOST_NAME"
fi

banner "Committing the VM to a raw disk"

step "Copy the OverlayBD block device to a $((COMMIT_SIZE / 1024 / 1024)) MiB raw disk in ${LOCAL_POOL_NAME}"
run qarax vm commit "$VM_NAME" --storage-pool "$LOCAL_POOL_NAME" --size "$COMMIT_SIZE"

banner "Verifying the commit"

step "VM after commit"
run qarax vm get "$VM_NAME"
IMAGE_REF_AFTER="$(qarax vm get "$VM_NAME" -o json | jq -r '.image_ref // empty')"
[[ -z "$IMAGE_REF_AFTER" ]] || die "image_ref was not cleared after commit (got: ${IMAGE_REF_AFTER})"
ok "image_ref is cleared: the VM no longer depends on the OCI image"

step "Committed disk"
run qarax storage-object list --pool "$LOCAL_POOL_NAME"
OBJ_TYPE="$(qarax storage-object list --pool "$LOCAL_POOL_NAME" -o json |
	jq -r --arg n "committed-${VM_ID}" '.[] | select(.name == $n) | .object_type')"
[[ "$OBJ_TYPE" == "disk" ]] ||
	die "Expected a disk object committed-${VM_ID} in ${LOCAL_POOL_NAME}, found '${OBJ_TYPE}'"
ok "committed-${VM_ID} is a raw disk in ${LOCAL_POOL_NAME}"

banner "Booting the committed VM"
run qarax vm start "$VM_NAME"
STATUS="$(vm_status "$VM_NAME")"
[[ "$STATUS" == "running" ]] || die "${VM_NAME} is '${STATUS}' after start (wanted running)"
ok "${VM_NAME} runs from its committed raw disk"

banner "Demo Complete"
echo "VM ${VM_NAME} was converted from an OverlayBD-backed OCI image to a standalone"
echo "raw disk in pool ${LOCAL_POOL_NAME}, and boots from it. Explore it:"
echo "  qarax vm get ${VM_NAME}"
echo "  qarax storage-object list --pool ${LOCAL_POOL_NAME}"
echo ""
echo "To tear down:"
echo "  $0 --cleanup"
