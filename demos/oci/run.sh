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
#   - qarax stack running (./hack/run-local.sh)
#   - qarax CLI installed and on PATH
#   - Host registered and initialized (run-local.sh does this automatically)
#   - OverlayBD storage pool created (run-local.sh does this automatically)
#
# Usage:
#   ./demos/oci/run.sh                                          # defaults: Alpine, 1 vCPU, 256 MiB
#   ./demos/oci/run.sh --image docker.io/library/ubuntu:latest  # custom image
#   ./demos/oci/run.sh --name my-vm --vcpus 2 --memory 512      # custom VM config
#   ./demos/oci/run.sh --pool my-pool                           # specify storage pool
#

set -euo pipefail

# Defaults
VM_NAME="demo-oci-vm"
IMAGE_REF="public.ecr.aws/docker/library/alpine:latest"
OBJECT_NAME=""
POOL_NAME="overlaybd-pool"
VCPUS=1
MEMORY_MIB=256
SERVER="${QARAX_SERVER:-http://localhost:8000}"

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
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo ""
		echo "Options:"
		echo "  --name NAME          VM name (default: demo-oci-vm)"
		echo "  --image REF          OCI image reference (default: alpine)"
		echo "  --object-name NAME   Storage object name (default: derived from image)"
		echo "  --pool NAME          Storage pool name or ID (default: overlaybd-pool)"
		echo "  --vcpus N            Number of vCPUs (default: 1)"
		echo "  --memory MiB         Memory in MiB (default: 256)"
		echo "  --server URL         qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		exit 0
		;;
	*)
		echo "Unknown option: $1"
		exit 1
		;;
	esac
done

# Derive object name from image ref if not specified
if [[ -z "$OBJECT_NAME" ]]; then
	# e.g. "public.ecr.aws/docker/library/alpine:latest" -> "alpine-latest-obd"
	OBJECT_NAME="$(echo "$IMAGE_REF" | sed 's|.*/||; s/:/-/g')-obd"
fi

MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))
QARAX="qarax --server $SERVER"

echo "=== qarax OCI VM Demo ==="
echo ""
echo "Image:   $IMAGE_REF"
echo "VM:      $VM_NAME"
echo "Pool:    $POOL_NAME"
echo "vCPUs:   $VCPUS"
echo "Memory:  ${MEMORY_MIB} MiB"
echo ""

# Step 1: Import OCI image into the storage pool
echo "--- Step 1: Import OCI image into storage pool ---"
echo "\$ qarax storage-pool import --pool $POOL_NAME --image-ref $IMAGE_REF --name $OBJECT_NAME"
$QARAX storage-pool import --pool "$POOL_NAME" --image-ref "$IMAGE_REF" --name "$OBJECT_NAME"
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

# Step 4: Start the VM
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
echo "  qarax vm delete $VM_NAME        # delete the VM"
