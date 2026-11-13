#!/usr/bin/env bash
#
# Demo: GPU passthrough with VFIO
#
# Boots a VM from an OCI container image (e.g. NVIDIA CUDA) with GPUs passed
# through as VFIO devices. qarax imports the image via OverlayBD and attaches
# the GPUs when the VM starts.
#
#   1. List the GPUs on a host
#   2. Create a VM with --image-ref and GPU scheduling flags
#   3. Start the VM
#   4. Check the VM holds the requested GPUs on the host the scheduler picked
#
# The VM (demo-gpu-vm) is deleted at exit, releasing its GPUs; KEEP=1 leaves
# it running.
#
# Prerequisites:
#   - qarax stack running (make run-local) with a host that has at least one
#     GPU bound to vfio-pci
#   - cargo (to build the qarax CLI) or a qarax CLI on PATH
#   - jq
#   - IOMMU enabled (intel_iommu=on iommu=pt in the host kernel cmdline)
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
#   ./demos/gpu-passthrough/run.sh                              # defaults (CUDA image, 1 GPU)
#   ./demos/gpu-passthrough/run.sh --gpu-count 2                # request 2 GPUs
#   ./demos/gpu-passthrough/run.sh --gpu-vendor nvidia          # filter by vendor
#   ./demos/gpu-passthrough/run.sh --image-ref nvcr.io/nvidia/cuda:12.6.3-devel-ubuntu24.04
#   ./demos/gpu-passthrough/run.sh --host my-node               # inspect GPUs on this host
#   KEEP=1 ./demos/gpu-passthrough/run.sh                       # leave the VM running

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-gpu-"
VM_NAME="${PREFIX}vm"
IMAGE_REF="docker.io/nvidia/cuda:12.6.3-base-ubuntu24.04"
GPU_COUNT=1
GPU_VENDOR=""
GPU_MODEL=""
MIN_VRAM=""
VCPUS=4
MEMORY_MIB=4096
HOST_NAME=""

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --image-ref REF        OCI image reference (default: docker.io/nvidia/cuda:12.6.3-base-ubuntu24.04)
  --gpu-count N          Number of GPUs to request (default: 1)
  --gpu-vendor VENDOR    GPU vendor filter (e.g. nvidia, amd)
  --gpu-model MODEL      GPU model filter
  --min-vram BYTES       Minimum GPU VRAM in bytes
  --vcpus N              Number of vCPUs (default: 4)
  --memory MiB           Memory in MiB (default: 4096)
  --host NAME            Host to list GPUs on (default: the first UP host)
  --server URL           qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)

Environment:
  KEEP=1                 Leave the VM running at exit (it holds its GPUs until deleted)
EOF
}

while [[ $# -gt 0 ]]; do
	case $1 in
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
		usage
		exit 0
		;;
	*)
		usage >&2
		die "Unknown option: $1"
		;;
	esac
done

# delete_image_vm VM: delete an --image-ref VM and the OverlayBD image object
# qarax created for it (overlaybd-<vm id>), which VM deletion leaves behind.
# Deleting the VM releases its GPUs.
delete_image_vm() {
	local id
	id="$(qarax vm get "$1" -o json | jq -r '.id')"
	qarax vm delete "$id" >/dev/null
	if exists storage-object "overlaybd-${id}"; then
		qarax storage-object delete "overlaybd-${id}" >/dev/null
	fi
}

cleanup() {
	if ((KEEP)); then
		echo ""
		info "KEEP=1: ${VM_NAME} keeps running and holds its GPUs. Delete it with: qarax vm delete ${VM_NAME}"
		return
	fi
	if exists vm "$VM_NAME"; then
		step "Deleting ${VM_NAME} (releases its GPUs)"
		delete_image_vm "$VM_NAME"
	fi
}

require_server
require_cli

banner "qarax GPU Passthrough Demo"
info "VM:         ${VM_NAME} (${VCPUS} vCPU, ${MEMORY_MIB} MiB)"
info "Image:      ${IMAGE_REF}"
info "GPU count:  ${GPU_COUNT}${GPU_VENDOR:+, vendor ${GPU_VENDOR}}${GPU_MODEL:+, model ${GPU_MODEL}}"

# A VM left by an interrupted or KEEP=1 run holds GPUs: delete it first.
if exists vm "$VM_NAME"; then
	delete_image_vm "$VM_NAME"
fi
trap cleanup EXIT

step "1. GPUs on the host"
[[ -n "$HOST_NAME" ]] || HOST_NAME="$(first_up_host)"
run qarax host gpus "$HOST_NAME"
FREE_GPUS="$(qarax host gpus "$HOST_NAME" -o json | jq '[.[] | select(.vm_id == null)] | length')"
if ((FREE_GPUS < GPU_COUNT)); then
	warn "Host '${HOST_NAME}' has ${FREE_GPUS} free GPU(s), ${GPU_COUNT} requested."
	warn "Scheduling fails unless another host has enough vfio-pci bound GPUs."
fi

GPU_FLAGS=(--gpu-count "$GPU_COUNT")
if [[ -n "$GPU_VENDOR" ]]; then
	GPU_FLAGS+=(--gpu-vendor "$GPU_VENDOR")
fi
if [[ -n "$GPU_MODEL" ]]; then
	GPU_FLAGS+=(--gpu-model "$GPU_MODEL")
fi
if [[ -n "$MIN_VRAM" ]]; then
	GPU_FLAGS+=(--min-vram "$MIN_VRAM")
fi

step "2. Create the GPU VM (the CLI waits for the image import)"
run qarax vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "${MEMORY_MIB}MiB" \
	--image-ref "$IMAGE_REF" "${GPU_FLAGS[@]}"

step "3. Start the VM"
run qarax vm start "$VM_NAME"
run qarax vm get "$VM_NAME"

step "4. GPU allocation on the host the scheduler picked"
VM_JSON="$(qarax vm get "$VM_NAME" -o json)"
VM_ID="$(jq -r '.id' <<<"$VM_JSON")"
VM_HOST="$(qarax host get "$(jq -r '.host_id' <<<"$VM_JSON")" -o json | jq -r '.name')"
run qarax host gpus "$VM_HOST"
HELD="$(qarax host gpus "$VM_HOST" -o json | jq --arg id "$VM_ID" '[.[] | select(.vm_id == $id)] | length')"
((HELD == GPU_COUNT)) || die "${VM_NAME} holds ${HELD} GPU(s), ${GPU_COUNT} requested"
ok "${VM_NAME} is running on ${VM_HOST} with ${HELD} GPU(s) passed through via VFIO"
info "Inside the guest (KEEP=1 keeps it running), lspci lists them as PCI devices."
