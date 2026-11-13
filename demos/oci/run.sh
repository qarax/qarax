#!/usr/bin/env bash
#
# Demo: Run a VM from an OCI container image
#
# The "OCI disk-first" workflow:
#   1. Import an OCI image into the shared OverlayBD storage pool
#   2. Create a VM with the imported image as its boot disk (guest agent enabled)
#   3. Start the VM
#   4. Read /etc/os-release inside the guest to show it booted from the image
#
# The VM (demo-oci-vm) is left running. A re-run replaces it and reuses the
# imported image; --cleanup deletes both.
#
# Prerequisites:
#   - qarax stack running (make run-local), or let the script start it.
#     make run-local registers the host and creates overlaybd-pool.
#   - cargo (to build the qarax CLI) or a qarax CLI on PATH
#   - jq
#
# Usage:
#   ./demos/oci/run.sh                                          # Alpine, 1 vCPU, 256 MiB
#   ./demos/oci/run.sh --image docker.io/library/ubuntu:latest  # custom image
#   ./demos/oci/run.sh --vcpus 2 --memory 512                   # more resources
#   ./demos/oci/run.sh --pool my-pool                           # another OverlayBD pool
#   ./demos/oci/run.sh --cleanup                                # delete the VM and imported object
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-oci-"
VM_NAME="${PREFIX}vm"
IMAGE_REF="public.ecr.aws/docker/library/alpine:latest"
OBJECT_NAME=""
POOL_NAME="overlaybd-pool"
VCPUS=1
MEMORY_MIB=256
CLEANUP=0
GUEST_AGENT_TIMEOUT=60

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --image REF          OCI image reference (default: public.ecr.aws/docker/library/alpine:latest)
  --object-name NAME   Storage object name (default: derived from the image, e.g. alpine-latest-obd)
  --pool NAME          OverlayBD storage pool name or ID (default: overlaybd-pool)
  --vcpus N            Number of vCPUs (default: 1)
  --memory MiB         Memory in MiB (default: 256)
  --server URL         qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)
  --cleanup            Delete the demo VM and the imported storage object, then exit
EOF
}

while [[ $# -gt 0 ]]; do
	case $1 in
	--image)
		IMAGE_REF="$2"
		shift 2
		;;
	--object-name)
		OBJECT_NAME="$2"
		shift 2
		;;
	--pool)
		POOL_NAME="$2"
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
	--server)
		SERVER="$2"
		shift 2
		;;
	--cleanup)
		CLEANUP=1
		shift
		;;
	--help | -h)
		usage
		exit 0
		;;
	*)
		usage >&2
		die "Unknown option: $1"
		;;
	esac
done

if [[ -z "$OBJECT_NAME" ]]; then
	# e.g. public.ecr.aws/docker/library/alpine:latest -> alpine-latest-obd
	OBJECT_NAME="$(sed 's|.*/||; s/[:@]/-/g' <<<"$IMAGE_REF")-obd"
fi

if ((CLEANUP)); then
	if ! server_reachable; then
		info "qarax server not reachable at ${SERVER}: nothing to clean up."
		exit 0
	fi
	require_auth
	require_cli
	step "Deleting OCI demo resources"
	sweep vm "$PREFIX"
	if exists storage-object "$OBJECT_NAME"; then
		run qarax storage-object delete "$OBJECT_NAME"
	fi
	ok "Done"
	exit 0
fi

ensure_stack
require_cli

banner "qarax OCI VM Demo"
info "Image:   ${IMAGE_REF}"
info "Object:  ${OBJECT_NAME}"
info "Pool:    ${POOL_NAME}"
info "VM:      ${VM_NAME} (${VCPUS} vCPU, ${MEMORY_MIB} MiB)"

exists storage-pool "$POOL_NAME" ||
	die "Storage pool '${POOL_NAME}' not found. Run 'make run-local' (it creates overlaybd-pool) or pass --pool."

# The VM name belongs to this demo: replace the VM from a previous run.
sweep vm "$PREFIX"

step "1. Import the OCI image into ${POOL_NAME}"
if exists storage-object "$OBJECT_NAME"; then
	ok "${OBJECT_NAME} is already imported, reusing it"
else
	run qarax storage-pool import --pool "$POOL_NAME" --image-ref "$IMAGE_REF" --name "$OBJECT_NAME"
fi

step "2. Create the VM with the imported image as its boot disk"
# --root-disk lets the scheduler pick a host that has the image's pool; the
# guest agent lets the demo look inside the VM.
run qarax vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "${MEMORY_MIB}MiB" \
	--root-disk "$OBJECT_NAME" --guest-agent

step "3. Start the VM"
run qarax vm start "$VM_NAME"
run qarax vm get "$VM_NAME"

step "4. Look inside the guest"
wait_until "$GUEST_AGENT_TIMEOUT" guest_agent_ready "$VM_NAME" ||
	die "The guest agent in ${VM_NAME} did not answer within ${GUEST_AGENT_TIMEOUT}s.\nWatch the boot with: qarax vm attach ${VM_NAME}"
run qarax vm exec "$VM_NAME" -- cat /etc/os-release
ok "${VM_NAME} booted from ${IMAGE_REF}"

banner "Done"
echo "VM ${VM_NAME} is running. Useful commands:"
echo "  qarax vm exec ${VM_NAME} -- ps   # run a command in the guest"
echo "  qarax vm attach ${VM_NAME}       # interactive serial console (Ctrl+] detaches)"
echo "  qarax vm stop ${VM_NAME}         # stop the VM"
echo "  $0 --cleanup   # delete the VM and the imported image"
