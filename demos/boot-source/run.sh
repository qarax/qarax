#!/usr/bin/env bash
#
# Demo: Run a VM from a kernel + rootfs (non-OCI boot source)
#
# This script demonstrates the traditional boot source workflow:
#   1. Create a local storage pool
#   2. Transfer a kernel (and optionally initramfs) into the pool
#   3. Create a boot source referencing the kernel/initramfs
#   4. Create and start a VM using the boot source
#
# Prerequisites:
#   - qarax stack running (./hack/run-local.sh --with-vm)
#   - qarax CLI installed and on PATH
#   - Host registered and initialized (run-local.sh does this automatically)
#   - Kernel and rootfs/initramfs available on the qarax-node filesystem
#     (run-local.sh --with-vm provides these at the default paths below)
#
# Usage:
#   ./demos/boot-source/run.sh                                     # use defaults from run-local.sh --with-vm
#   ./demos/boot-source/run.sh --kernel /path/to/vmlinux           # custom kernel path
#   ./demos/boot-source/run.sh --initramfs /path/to/initramfs.gz   # custom initramfs
#   ./demos/boot-source/run.sh --cmdline "console=ttyS0 root=/dev/vda"
#

set -euo pipefail

# Defaults (match hack/run-local.sh --with-vm paths inside the qarax-node container)
VM_NAME="demo-bootsrc-vm"
POOL_NAME="demo-local-pool"
POOL_PATH="/var/lib/qarax/images"
BOOT_SOURCE_NAME="demo-boot"
KERNEL_PATH="/var/lib/qarax/images/vmlinux"
KERNEL_NAME="demo-kernel"
INITRAMFS_PATH="/var/lib/qarax/production-images/boot-initramfs.gz"
INITRAMFS_NAME="demo-initramfs"
CMDLINE="console=ttyS0"
VCPUS=1
MEMORY_MIB=256
MAC_ADDRESS="52:54:00:12:34:56"
SERVER="${QARAX_SERVER:-http://localhost:8000}"

# Parse arguments
while [[ $# -gt 0 ]]; do
	case $1 in
	--name)
		VM_NAME="$2"
		shift 2
		;;
	--pool-name)
		POOL_NAME="$2"
		shift 2
		;;
	--pool-path)
		POOL_PATH="$2"
		shift 2
		;;
	--kernel)
		KERNEL_PATH="$2"
		shift 2
		;;
	--initramfs)
		INITRAMFS_PATH="$2"
		shift 2
		;;
	--no-initramfs)
		INITRAMFS_PATH=""
		shift
		;;
	--cmdline)
		CMDLINE="$2"
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
	--mac)
		MAC_ADDRESS="$2"
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
		echo "  --name NAME            VM name (default: demo-bootsrc-vm)"
		echo "  --pool-name NAME       Storage pool name (default: demo-local-pool)"
		echo "  --pool-path PATH       Local path on qarax-node for pool (default: /var/lib/qarax/images)"
		echo "  --kernel PATH          Kernel path on qarax-node (default: /var/lib/qarax/images/vmlinux)"
		echo "  --initramfs PATH       Initramfs path on qarax-node (default: boot-initramfs.gz)"
		echo "  --no-initramfs         Skip initramfs"
		echo "  --cmdline PARAMS       Kernel command line (default: console=ttyS0)"
		echo "  --vcpus N              Number of vCPUs (default: 1)"
		echo "  --memory MiB           Memory in MiB (default: 256)"
		echo "  --mac ADDRESS          MAC address for the NIC (default: 52:54:00:12:34:56)"
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

echo "=== qarax Boot Source VM Demo ==="
echo ""
echo "Kernel:     $KERNEL_PATH"
if [[ -n "$INITRAMFS_PATH" ]]; then
	echo "Initramfs:  $INITRAMFS_PATH"
fi
echo "Cmdline:    $CMDLINE"
echo "VM:         $VM_NAME"
echo "vCPUs:      $VCPUS"
echo "Memory:     ${MEMORY_MIB} MiB"
echo ""

# Step 1: Create a local storage pool (idempotent — reuses if exists)
echo "--- Step 1: Create local storage pool ---"
echo "\$ qarax storage-pool create --name $POOL_NAME --pool-type local --config '{\"path\":\"$POOL_PATH\"}' --attach-all-hosts"
$QARAX storage-pool create --name "$POOL_NAME" --pool-type local \
	--config "{\"path\":\"$POOL_PATH\"}" --attach-all-hosts 2>/dev/null || echo "(pool may already exist, continuing)"
echo ""

# Step 2: Transfer kernel into the pool
echo "--- Step 2: Transfer kernel ---"
echo "\$ qarax transfer create --pool $POOL_NAME --name $KERNEL_NAME --source $KERNEL_PATH --object-type kernel"
$QARAX transfer create --pool "$POOL_NAME" --name "$KERNEL_NAME" \
	--source "$KERNEL_PATH" --object-type kernel 2>/dev/null || echo "(kernel may already exist, continuing)"
echo ""

# Step 3: Transfer initramfs (if provided)
INITRAMFS_FLAG=""
if [[ -n "$INITRAMFS_PATH" ]]; then
	echo "--- Step 3: Transfer initramfs ---"
	echo "\$ qarax transfer create --pool $POOL_NAME --name $INITRAMFS_NAME --source $INITRAMFS_PATH --object-type initrd"
	$QARAX transfer create --pool "$POOL_NAME" --name "$INITRAMFS_NAME" \
		--source "$INITRAMFS_PATH" --object-type initrd 2>/dev/null || echo "(initramfs may already exist, continuing)"
	INITRAMFS_FLAG="--initrd $INITRAMFS_NAME"
	echo ""
fi

# Step 4: Create a boot source
echo "--- Step 4: Create boot source ---"
echo "\$ qarax boot-source create --name $BOOT_SOURCE_NAME --kernel $KERNEL_NAME $INITRAMFS_FLAG --params \"$CMDLINE\""
# shellcheck disable=SC2086
$QARAX boot-source create --name "$BOOT_SOURCE_NAME" --kernel "$KERNEL_NAME" \
	$INITRAMFS_FLAG --params "$CMDLINE" 2>/dev/null || echo "(boot source may already exist, continuing)"
echo ""

# Step 5: Create the VM with the boot source
echo "--- Step 5: Create VM ---"
echo "\$ qarax vm create --name $VM_NAME --vcpus $VCPUS --memory $MEMORY_BYTES --boot-source $BOOT_SOURCE_NAME"
$QARAX vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "$MEMORY_BYTES" \
	--boot-source "$BOOT_SOURCE_NAME"
echo ""

# Step 6: Start the VM
echo "--- Step 6: Start VM ---"
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
echo "  qarax boot-source list          # list boot sources"
echo "  qarax storage-object list       # list storage objects"
