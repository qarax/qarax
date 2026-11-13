#!/usr/bin/env bash
#
# Demo: Run a VM from a kernel + initramfs (non-OCI boot source)
#
# The traditional direct-boot workflow:
#   1. Create a local storage pool on the host
#   2. Copy a kernel (and optionally an initramfs) into the pool
#   3. Create a boot source referencing them
#   4. Create and start a VM using the boot source (with the guest agent enabled)
#   5. Run commands in the guest to show it booted with the boot source's
#      kernel command line
#
# The VM (demo-bootsrc-vm) is left running. A re-run replaces the VM, the boot
# source and the copied kernel/initramfs, and reuses the pool.
#
# Prerequisites:
#   - qarax stack running (make run-local), or let the script start it
#   - cargo (to build the qarax CLI) or a qarax CLI on PATH
#   - jq
#   - Kernel and initramfs on the host's filesystem (the local qarax-node image
#     ships them at the default paths below). Step 5 needs an initramfs whose
#     init is qarax-init, like the default one.
#
# Usage:
#   ./demos/boot-source/run.sh                                     # built-in kernel + test initramfs
#   ./demos/boot-source/run.sh --kernel /path/to/vmlinux           # custom kernel path
#   ./demos/boot-source/run.sh --initramfs /path/to/initramfs.gz   # custom initramfs
#   ./demos/boot-source/run.sh --cmdline "console=ttyS0 quiet"     # custom kernel command line
#   ./demos/boot-source/run.sh --cleanup                           # delete demo resources
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

# Defaults (the paths match the files baked into the local qarax-node image)
PREFIX="demo-bootsrc-"
VM_NAME="${PREFIX}vm"
BOOT_SOURCE_NAME="${PREFIX}boot"
KERNEL_NAME="${PREFIX}kernel"
INITRAMFS_NAME="${PREFIX}initramfs"
POOL_NAME="${PREFIX}pool"
POOL_PATH="/var/lib/qarax/images"
HOST_NAME="${QARAX_HOST:-}"
KERNEL_PATH="/var/lib/qarax/images/vmlinux"
INITRAMFS_PATH="/var/lib/qarax/images/test-initramfs.gz"
CMDLINE="console=ttyS0"
VCPUS=1
MEMORY_MIB=256
CLEANUP=0
GUEST_AGENT_TIMEOUT=60

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --pool-name NAME       Storage pool name (default: demo-bootsrc-pool)
  --pool-path PATH       Pool directory on the host (default: /var/lib/qarax/images)
  --host NAME            Host to attach the pool to (default: \$QARAX_HOST or the first UP host)
  --kernel PATH          Kernel path on the host (default: /var/lib/qarax/images/vmlinux)
  --initramfs PATH       Initramfs path on the host (default: /var/lib/qarax/images/test-initramfs.gz)
  --no-initramfs         Boot without an initramfs
  --cmdline PARAMS       Kernel command line (default: console=ttyS0)
  --vcpus N              Number of vCPUs (default: 1)
  --memory MiB           Memory in MiB (default: 256)
  --server URL           qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)
  --cleanup              Delete the demo resources (VM, boot source, objects, pool), then exit
EOF
}

while [[ $# -gt 0 ]]; do
	case $1 in
	--pool-name)
		POOL_NAME="$2"
		shift 2
		;;
	--pool-path)
		POOL_PATH="$2"
		shift 2
		;;
	--host)
		HOST_NAME="$2"
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

# Delete what a previous run created, except the pool: VMs first, then the
# boot source that references the copied kernel/initramfs, then the copies.
sweep_demo() {
	sweep vm "$PREFIX"
	sweep boot-source "$PREFIX"
	sweep storage-object "$PREFIX"
}

if ((CLEANUP)); then
	if ! server_reachable; then
		info "qarax server not reachable at ${SERVER}: nothing to clean up."
		exit 0
	fi
	require_auth
	require_cli
	step "Deleting boot-source demo resources"
	sweep_demo
	if exists storage-pool "$POOL_NAME"; then
		run qarax storage-pool delete "$POOL_NAME"
	fi
	ok "Done"
	exit 0
fi

ensure_stack
require_cli

banner "qarax Boot Source VM Demo"
info "Kernel:     ${KERNEL_PATH}"
info "Initramfs:  ${INITRAMFS_PATH:-none}"
info "Cmdline:    ${CMDLINE}"
info "VM:         ${VM_NAME} (${VCPUS} vCPU, ${MEMORY_MIB} MiB)"

sweep_demo

step "1. Local storage pool"
if exists storage-pool "$POOL_NAME"; then
	ok "${POOL_NAME} exists, reusing it"
else
	[[ -n "$HOST_NAME" ]] || HOST_NAME="$(first_up_host)"
	run qarax storage-pool create --name "$POOL_NAME" --pool-type local \
		--path "$POOL_PATH" --host "$HOST_NAME"
fi

step "2. Copy the kernel into the pool"
run qarax transfer create --pool "$POOL_NAME" --name "$KERNEL_NAME" \
	--source "$KERNEL_PATH" --object-type kernel --wait

BOOT_SOURCE_ARGS=(--name "$BOOT_SOURCE_NAME" --kernel "$KERNEL_NAME" --params "$CMDLINE")
if [[ -n "$INITRAMFS_PATH" ]]; then
	step "3. Copy the initramfs into the pool"
	run qarax transfer create --pool "$POOL_NAME" --name "$INITRAMFS_NAME" \
		--source "$INITRAMFS_PATH" --object-type initrd --wait
	BOOT_SOURCE_ARGS+=(--initrd "$INITRAMFS_NAME")
fi

step "4. Create the boot source"
run qarax boot-source create "${BOOT_SOURCE_ARGS[@]}"

step "5. Create the VM (the guest agent lets the demo look inside it)"
run qarax vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "${MEMORY_MIB}MiB" \
	--boot-source "$BOOT_SOURCE_NAME" --guest-agent

step "6. Start the VM"
run qarax vm start "$VM_NAME"
run qarax vm get "$VM_NAME"

step "7. Look inside the guest"
wait_until "$GUEST_AGENT_TIMEOUT" guest_agent_ready "$VM_NAME" ||
	die "The guest agent in ${VM_NAME} did not answer within ${GUEST_AGENT_TIMEOUT}s.\nWatch the boot with: qarax vm attach ${VM_NAME}"
run qarax vm exec "$VM_NAME" -- uname -a
run qarax vm exec "$VM_NAME" -- cat /proc/cmdline
ok "${VM_NAME} booted from boot source ${BOOT_SOURCE_NAME}"

banner "Done"
echo "VM ${VM_NAME} is running. Useful commands:"
echo "  qarax vm exec ${VM_NAME} -- ls /   # run a command in the guest"
echo "  qarax vm attach ${VM_NAME}         # interactive serial console (Ctrl+] detaches)"
echo "  qarax boot-source list             # list boot sources"
echo "  qarax storage-object list          # list storage objects"
echo "  $0 --cleanup   # delete all demo resources"
