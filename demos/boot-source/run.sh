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
#   - qarax stack running (make run-local), or let the script start it
#   - qarax CLI on PATH or built under target/ (make build)
#   - Host registered and initialized (run-local.sh does this automatically)
#   - Kernel and initramfs available on the qarax-node filesystem
#     (the local qarax-node image ships them at the default paths below)
#
# Usage:
#   ./demos/boot-source/run.sh                                     # built-in kernel + test initramfs
#   ./demos/boot-source/run.sh --kernel /path/to/vmlinux           # custom kernel path
#   ./demos/boot-source/run.sh --initramfs /path/to/initramfs.gz   # custom initramfs
#   ./demos/boot-source/run.sh --cmdline "console=ttyS0 root=/dev/vda"
#   ./demos/boot-source/run.sh --cleanup                           # delete demo resources
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

# Defaults (match the paths baked into the local qarax-node container image)
VM_NAME="demo-bootsrc-vm"
HOST_NAME="${QARAX_HOST:-local-node}"
POOL_NAME="demo-local-pool"
POOL_PATH="/var/lib/qarax/images"
BOOT_SOURCE_NAME="demo-boot"
KERNEL_PATH="/var/lib/qarax/images/vmlinux"
KERNEL_NAME="demo-kernel"
INITRAMFS_PATH="/var/lib/qarax/images/test-initramfs.gz"
INITRAMFS_NAME="demo-initramfs"
CMDLINE="console=ttyS0"
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
	--host)
		HOST_NAME="$2"
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
		echo "  --name NAME            VM name (default: demo-bootsrc-vm)"
		echo "  --pool-name NAME       Storage pool name (default: demo-local-pool)"
		echo "  --pool-path PATH       Local path on qarax-node for pool (default: /var/lib/qarax/images)"
		echo "  --kernel PATH          Kernel path on qarax-node (default: /var/lib/qarax/images/vmlinux)"
		echo "  --initramfs PATH       Initramfs path on qarax-node (default: test-initramfs.gz)"
		echo "  --no-initramfs         Skip initramfs"
		echo "  --cmdline PARAMS       Kernel command line (default: console=ttyS0)"
		echo "  --vcpus N              Number of vCPUs (default: 1)"
		echo "  --memory MiB           Memory in MiB (default: 256)"
		echo "  --host NAME            Host to attach the pool to (default: \$QARAX_HOST or local-node)"
		echo "  --server URL           qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --cleanup              Delete demo resources and exit"
		exit 0
		;;
	*)
		echo "Unknown option: $1" >&2
		exit 1
		;;
	esac
done

MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found. Run 'make build' or add it to PATH."
QARAX="$QARAX_BIN --server $SERVER"

if [[ "$CLEANUP" -eq 1 ]]; then
	if ! curl -sf --max-time 3 "${SERVER}/" >/dev/null 2>&1; then
		echo "  Stack not running — nothing to clean up."
		exit 0
	fi
	require_auth "$SERVER"
	echo "=== Cleaning up boot-source demo resources ==="
	timeout 60 $QARAX vm force-stop --wait "$VM_NAME" >/dev/null 2>&1 || true
	$QARAX vm delete "$VM_NAME" 2>/dev/null || true
	$QARAX boot-source delete "$BOOT_SOURCE_NAME" 2>/dev/null || true
	$QARAX storage-object delete "$KERNEL_NAME" 2>/dev/null || true
	$QARAX storage-object delete "$INITRAMFS_NAME" 2>/dev/null || true
	$QARAX storage-pool delete "$POOL_NAME" 2>/dev/null || true
	echo "Done."
	exit 0
fi

ensure_stack "$SERVER"

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

if $QARAX vm get "$VM_NAME" >/dev/null 2>&1; then
	die "VM '$VM_NAME' already exists. Run '$0 --cleanup' first or pass --name."
fi

# Step 1: Create a local storage pool (reused if it already exists)
echo "--- Step 1: Create local storage pool ---"
if $QARAX storage-pool get "$POOL_NAME" >/dev/null 2>&1; then
	echo "(storage pool '$POOL_NAME' already exists, reusing it)"
else
	echo "\$ qarax storage-pool create --name $POOL_NAME --pool-type local --path $POOL_PATH --host $HOST_NAME"
	$QARAX storage-pool create --name "$POOL_NAME" --pool-type local \
		--path "$POOL_PATH" --host "$HOST_NAME"
fi
echo ""

# Copy a file on qarax-node into the pool as a storage object, unless an
# object with that name already exists (e.g. from a previous run).
transfer_object() {
	local name="$1" source="$2" object_type="$3"
	if $QARAX storage-object get "$name" >/dev/null 2>&1; then
		echo "(storage object '$name' already exists, reusing it)"
		return 0
	fi
	echo "\$ qarax transfer create --pool $POOL_NAME --name $name --source $source --object-type $object_type --wait"
	$QARAX transfer create --pool "$POOL_NAME" --name "$name" \
		--source "$source" --object-type "$object_type" --wait
}

# Step 2: Transfer kernel into the pool
echo "--- Step 2: Transfer kernel ---"
transfer_object "$KERNEL_NAME" "$KERNEL_PATH" kernel
echo ""

# Step 3: Transfer initramfs (if provided)
INITRAMFS_ARGS=()
if [[ -n "$INITRAMFS_PATH" ]]; then
	echo "--- Step 3: Transfer initramfs ---"
	transfer_object "$INITRAMFS_NAME" "$INITRAMFS_PATH" initrd
	INITRAMFS_ARGS=(--initrd "$INITRAMFS_NAME")
	echo ""
fi

# Step 4: Create a boot source. Recreate it if it exists so that the current
# --cmdline / --no-initramfs settings take effect.
echo "--- Step 4: Create boot source ---"
if $QARAX boot-source get "$BOOT_SOURCE_NAME" >/dev/null 2>&1; then
	echo "(boot source '$BOOT_SOURCE_NAME' already exists, recreating it)"
	$QARAX boot-source delete "$BOOT_SOURCE_NAME" >/dev/null
fi
echo "\$ qarax boot-source create --name $BOOT_SOURCE_NAME --kernel $KERNEL_NAME ${INITRAMFS_ARGS[*]} --params \"$CMDLINE\""
$QARAX boot-source create --name "$BOOT_SOURCE_NAME" --kernel "$KERNEL_NAME" \
	"${INITRAMFS_ARGS[@]}" --params "$CMDLINE"
echo ""

# Step 5: Create the VM with the boot source
echo "--- Step 5: Create VM ---"
echo "\$ qarax vm create --name $VM_NAME --vcpus $VCPUS --memory $MEMORY_BYTES --boot-source $BOOT_SOURCE_NAME"
$QARAX vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "$MEMORY_BYTES" \
	--boot-source "$BOOT_SOURCE_NAME"
echo ""

# Step 6: Start the VM (the CLI waits for the start job to finish)
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
echo "  qarax boot-source list          # list boot sources"
echo "  qarax storage-object list       # list storage objects"
echo "  $0 --cleanup   # delete all demo resources"
