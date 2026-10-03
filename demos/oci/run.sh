#!/usr/bin/env bash
#
# Demo: Run a VM from an OCI container image
#
# This script demonstrates the "OCI disk-first" workflow:
#   1. Import an OCI image into an OverlayBD storage pool
#   2. Create a VM
#   3. Attach the imported image as a boot disk
#   4. Start the VM
#
# Prerequisites:
#   - qarax stack running (make run-local), or let the script start it
#   - qarax CLI on PATH or built under target/ (make build)
#   - Host registered and initialized (run-local.sh does this automatically)
#   - OverlayBD storage pool created (run-local.sh does this automatically)
#
# Usage:
#   ./demos/oci/run.sh                                          # defaults: Alpine, 1 vCPU, 256 MiB
#   ./demos/oci/run.sh --image docker.io/library/ubuntu:latest  # custom image
#   ./demos/oci/run.sh --name my-vm --vcpus 2 --memory 512      # custom VM config
#   ./demos/oci/run.sh --pool my-pool                           # specify storage pool
#   ./demos/oci/run.sh --cleanup                                # delete demo VM + imported object
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

# Defaults
VM_NAME="demo-oci-vm"
IMAGE_REF="public.ecr.aws/docker/library/alpine:latest"
OBJECT_NAME=""
POOL_NAME="overlaybd-pool"
VCPUS=1
MEMORY_MIB=256
SERVER="${QARAX_SERVER:-http://localhost:8000}"
CLEANUP=0

# Parse arguments
while [[ $# -gt 0 ]]; do
	case $1 in
	--name)
		VM_NAME="$2"
		shift 2
		;;
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
		echo "Usage: $0 [OPTIONS]"
		echo ""
		echo "Options:"
		echo "  --name NAME          VM name (default: demo-oci-vm)"
		echo "  --image REF          OCI image reference (default: public.ecr.aws/docker/library/alpine:latest)"
		echo "  --object-name NAME   Storage object name (default: derived from image)"
		echo "  --pool NAME          Storage pool name or ID (default: overlaybd-pool)"
		echo "  --vcpus N            Number of vCPUs (default: 1)"
		echo "  --memory MiB         Memory in MiB (default: 256)"
		echo "  --server URL         qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --cleanup            Delete the demo VM and imported storage object, then exit"
		exit 0
		;;
	*)
		echo "Unknown option: $1" >&2
		exit 1
		;;
	esac
done

# Derive object name from image ref if not specified
if [[ -z "$OBJECT_NAME" ]]; then
	# e.g. "public.ecr.aws/docker/library/alpine:latest" -> "alpine-latest-obd"
	OBJECT_NAME="$(echo "$IMAGE_REF" | sed 's|.*/||; s/[:@]/-/g')-obd"
fi

MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found. Run 'make build' or add it to PATH."
QARAX="$QARAX_BIN --server $SERVER"

if [[ "$CLEANUP" -eq 1 ]]; then
	if ! curl -sf --max-time 3 "${SERVER}/" >/dev/null 2>&1; then
		echo "Stack not running — nothing to clean up."
		exit 0
	fi
	require_auth "$SERVER"
	echo "=== Cleaning up OCI demo resources ==="
	timeout 60 $QARAX vm force-stop --wait "$VM_NAME" >/dev/null 2>&1 || true
	$QARAX vm delete "$VM_NAME" 2>/dev/null || true
	$QARAX storage-object delete "$OBJECT_NAME" 2>/dev/null || true
	echo "Done."
	exit 0
fi

ensure_stack "$SERVER"

echo "=== qarax OCI VM Demo ==="
echo ""
echo "Image:   $IMAGE_REF"
echo "Object:  $OBJECT_NAME"
echo "VM:      $VM_NAME"
echo "Pool:    $POOL_NAME"
echo "vCPUs:   $VCPUS"
echo "Memory:  ${MEMORY_MIB} MiB"
echo ""

$QARAX storage-pool get "$POOL_NAME" >/dev/null 2>&1 ||
	die "Storage pool '$POOL_NAME' not found. Run 'make run-local' (creates overlaybd-pool) or pass --pool."
if $QARAX vm get "$VM_NAME" >/dev/null 2>&1; then
	die "VM '$VM_NAME' already exists. Run '$0 --cleanup' first or pass --name."
fi

# Step 1: Import OCI image into the storage pool (skipped if already imported)
echo "--- Step 1: Import OCI image into storage pool ---"
if $QARAX storage-object get "$OBJECT_NAME" >/dev/null 2>&1; then
	echo "(storage object '$OBJECT_NAME' already exists, reusing it)"
else
	echo "\$ qarax storage-pool import --pool $POOL_NAME --image-ref $IMAGE_REF --name $OBJECT_NAME"
	$QARAX storage-pool import --pool "$POOL_NAME" --image-ref "$IMAGE_REF" --name "$OBJECT_NAME"
fi
echo ""

# Step 2: Create the VM
echo "--- Step 2: Create VM ---"
echo "\$ qarax vm create --name $VM_NAME --vcpus $VCPUS --memory $MEMORY_BYTES"
$QARAX vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "$MEMORY_BYTES"
echo ""

# Step 3: Attach the imported disk
echo "--- Step 3: Attach OCI disk to VM ---"
echo "\$ qarax vm attach-disk $VM_NAME --object $OBJECT_NAME"
$QARAX vm attach-disk "$VM_NAME" --object "$OBJECT_NAME"
echo ""

# Step 4: Start the VM (the CLI waits for the start job to finish)
echo "--- Step 4: Start VM ---"
echo "\$ qarax vm start $VM_NAME"
$QARAX vm start "$VM_NAME"
echo ""

# Show result
echo "--- VM Status ---"
$QARAX vm get "$VM_NAME"
echo ""

echo "=== Done ==="
echo ""
echo "Useful commands:"
echo "  qarax vm console $VM_NAME       # view boot log"
echo "  qarax vm attach $VM_NAME        # interactive console"
echo "  qarax vm stop $VM_NAME          # stop the VM"
echo "  $0 --cleanup        # delete the VM and imported object"
