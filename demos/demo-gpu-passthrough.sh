#!/usr/bin/env bash
#
# Demo: GPU passthrough with VFIO
#
# This script demonstrates GPU passthrough using qarax with an OCI container
# image (e.g. NVIDIA CUDA). qarax pulls the image, unpacks the rootfs, and
# boots it via virtiofs with the GPU passed through as a VFIO device.
#
#   1. List available GPUs on the host
#   2. Create a VM with --image-ref and GPU scheduling flags
#   3. Start the VM — image is pulled, virtiofsd started, VFIO devices attached
#
# Prerequisites:
#   - qarax stack running (make run-local)
#   - Host registered and initialized with at least one GPU bound to vfio-pci
#   - IOMMU enabled (intel_iommu=on iommu=pt in kernel cmdline)
#
# To enable IOMMU (Intel):
#   Add "intel_iommu=on iommu=pt" to GRUB_CMDLINE_LINUX in /etc/default/grub
#   sudo grub-mkconfig -o /boot/grub/grub.cfg && reboot
#
# To bind a GPU to vfio-pci:
#   # Find your GPU's PCI address and current driver
#   lspci -nnk | grep -A3 -i nvidia   # (or amd)
#
#   sudo modprobe vfio-pci
#   echo 0000:01:00.0 | sudo tee /sys/bus/pci/drivers/<current_driver>/unbind
#   echo <VENDOR_ID> <DEVICE_ID> | sudo tee /sys/bus/pci/drivers/vfio-pci/new_id
#
#   # Verify:
#   ls -la /dev/vfio/   # should show an IOMMU group number
#
# Usage:
#   ./demos/demo-gpu-passthrough.sh                              # defaults (CUDA image, 1 GPU)
#   ./demos/demo-gpu-passthrough.sh --gpu-count 2                # request 2 GPUs
#   ./demos/demo-gpu-passthrough.sh --gpu-vendor nvidia          # filter by vendor
#   ./demos/demo-gpu-passthrough.sh --image-ref nvcr.io/nvidia/cuda:12.6.3-devel-ubuntu24.04
#   ./demos/demo-gpu-passthrough.sh --host my-node               # specify host

set -euo pipefail

# Defaults
VM_NAME="demo-gpu-vm"
IMAGE_REF="docker.io/nvidia/cuda:12.6.3-base-ubuntu24.04"
GPU_COUNT=1
GPU_VENDOR=""
GPU_MODEL=""
MIN_VRAM=""
VCPUS=4
MEMORY_MIB=4096
HOST_NAME=""
SERVER="${QARAX_SERVER:-http://localhost:8000}"

# Parse arguments
while [[ $# -gt 0 ]]; do
	case $1 in
	--name)
		VM_NAME="$2"
		shift 2
		;;
	--image-ref)
		IMAGE_REF="$2"
		shift 2
		;;
	--gpu-count)
		GPU_COUNT="$2"
		shift 2
		;;
	--gpu-vendor)
		GPU_VENDOR="$2"
		shift 2
		;;
	--gpu-model)
		GPU_MODEL="$2"
		shift 2
		;;
	--min-vram)
		MIN_VRAM="$2"
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
	--host)
		HOST_NAME="$2"
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
		echo "  --name NAME            VM name (default: demo-gpu-vm)"
		echo "  --image-ref REF        OCI image reference (default: docker.io/nvidia/cuda:12.6.3-base-ubuntu24.04)"
		echo "  --gpu-count N          Number of GPUs to request (default: 1)"
		echo "  --gpu-vendor VENDOR    GPU vendor filter (e.g. nvidia, amd)"
		echo "  --gpu-model MODEL      GPU model filter"
		echo "  --min-vram BYTES       Minimum GPU VRAM in bytes"
		echo "  --vcpus N              Number of vCPUs (default: 4)"
		echo "  --memory MiB           Memory in MiB (default: 4096)"
		echo "  --host NAME            Host name to inspect GPUs on (default: first host)"
		echo "  --server URL           qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		exit 0
		;;
	*)
		echo "Unknown option: $1"
		exit 1
		;;
	esac
done

MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))
QARAX="qarax --server $SERVER"

echo "=== qarax GPU Passthrough Demo ==="
echo ""
echo "VM:         $VM_NAME"
echo "Image:      $IMAGE_REF"
echo "GPU count:  $GPU_COUNT"
if [[ -n "$GPU_VENDOR" ]]; then
	echo "GPU vendor: $GPU_VENDOR"
fi
if [[ -n "$GPU_MODEL" ]]; then
	echo "GPU model:  $GPU_MODEL"
fi
echo "vCPUs:      $VCPUS"
echo "Memory:     ${MEMORY_MIB} MiB"
echo ""

# Find a host with GPUs
echo "List available GPUs"
if [[ -z "$HOST_NAME" ]]; then
	FIRST_HOST=$($QARAX host list -o json | python3 -c "import json,sys; hosts=json.load(sys.stdin); print(hosts[0]['name'] if hosts else '')" 2>/dev/null || true)
	if [[ -n "$FIRST_HOST" ]]; then
		HOST_NAME="$FIRST_HOST"
	else
		echo "(no hosts found — register a host first)"
		exit 1
	fi
fi
echo "\$ qarax host gpus $HOST_NAME"
$QARAX host gpus "$HOST_NAME"
echo ""

# Build GPU flags
GPU_FLAGS="--gpu-count $GPU_COUNT"
if [[ -n "$GPU_VENDOR" ]]; then
	GPU_FLAGS="$GPU_FLAGS --gpu-vendor $GPU_VENDOR"
fi
if [[ -n "$GPU_MODEL" ]]; then
	GPU_FLAGS="$GPU_FLAGS --gpu-model $GPU_MODEL"
fi
if [[ -n "$MIN_VRAM" ]]; then
	GPU_FLAGS="$GPU_FLAGS --min-vram $MIN_VRAM"
fi

# Create VM with OCI image + GPU passthrough
echo "Create GPU VM"
echo "\$ qarax vm create --name $VM_NAME --vcpus $VCPUS --memory $MEMORY_BYTES --image-ref $IMAGE_REF $GPU_FLAGS"
# shellcheck disable=SC2086
$QARAX vm create --name "$VM_NAME" \
	--vcpus "$VCPUS" --memory "$MEMORY_BYTES" \
	--image-ref "$IMAGE_REF" \
	$GPU_FLAGS
echo ""

# Start the VM
echo "Start the VM"
echo "\$ qarax vm start $VM_NAME"
$QARAX vm start "$VM_NAME"
echo ""

# Show result
echo "--- VM Status ---"
$QARAX vm get "$VM_NAME"
echo ""

echo "--- GPU Allocation ---"
$QARAX host gpus "$HOST_NAME"
echo ""

echo "=== Done ==="
echo ""
echo "The VM is running with $GPU_COUNT GPU(s) passed through via VFIO."
echo "Inside the guest, 'lspci' will show the GPU as a PCI device."
echo ""
echo "Useful commands:"
echo "  qarax vm get $VM_NAME            # check VM status"
echo "  qarax host gpus $HOST_NAME       # check GPU allocation"
echo "  qarax vm stop $VM_NAME           # stop the VM (releases GPUs)"
echo "  qarax vm delete $VM_NAME         # delete the VM"
