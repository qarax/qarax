#!/usr/bin/env bash
#
# Demo: regular VM guest exec via qarax vm exec
#
# Creates a bootable Cloud Hypervisor VM template from the built-in demo kernel
# + initramfs, launches a VM with --guest-agent enabled, starts it, and runs a
# command inside the guest through the new regular-VM exec path.
#
# Prerequisites:
#   - qarax stack running (./hack/run-local.sh; started automatically if not up)
#   - qarax CLI on PATH or built under target/
#   - jq installed
#
# Usage:
#   ./demos/vm-exec/run.sh
#   ./demos/vm-exec/run.sh --server http://localhost:8000
#   ./demos/vm-exec/run.sh --keep
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
HOST_NAME=""
KEEP=0

RUN_SUFFIX="$(date +%s)-$$"
POOL_NAME="vm-exec-demo-pool-${RUN_SUFFIX}"
POOL_PATH="/var/lib/qarax/vm-exec-demo-${RUN_SUFFIX}"
KERNEL_PATH="/var/lib/qarax/images/vmlinux"
INITRAMFS_PATH="/var/lib/qarax/images/test-initramfs.gz"
KERNEL_NAME="vm-exec-demo-kernel-${RUN_SUFFIX}"
INITRAMFS_NAME="vm-exec-demo-initramfs-${RUN_SUFFIX}"
BOOT_SOURCE_NAME="vm-exec-demo-boot-${RUN_SUFFIX}"
TEMPLATE_NAME="vm-exec-demo-template-${RUN_SUFFIX}"
VM_NAME="vm-exec-demo-vm-${RUN_SUFFIX}"

VCPUS=1
MEMORY_BYTES=$((256 * 1024 * 1024))
EXEC_TIMEOUT=15
EXEC_READY_TIMEOUT=90
EXEC_PAYLOAD='printf vm-exec && uname -s'

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

while [[ $# -gt 0 ]]; do
	case "$1" in
	--server)
		SERVER="$2"
		shift 2
		;;
	--host)
		HOST_NAME="$2"
		shift 2
		;;
	--keep)
		KEEP=1
		shift
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo ""
		echo "Options:"
		echo "  --server URL   qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --host NAME    Host name or ID to attach the demo pool to"
		echo "  --keep         Leave the demo VM/template/pool in place for inspection"
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

if ! command -v jq >/dev/null 2>&1; then
	die "jq is required for this demo"
fi

if [[ -z "$(find_qarax_bin)" ]]; then
	echo "qarax CLI not found — building..."
	cargo build -p cli
fi

supports_guest_exec_cli() {
	local bin="$1"
	"$bin" vm create --help 2>/dev/null | grep -q -- '--guest-agent' &&
		"$bin" vm exec --help >/dev/null 2>&1
}

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"

if ! supports_guest_exec_cli "$QARAX_BIN"; then
	FALLBACK_RELEASE_BIN="${REPO_ROOT}/target/${MUSL_TARGET}/release/qarax"
	if [[ -x "$FALLBACK_RELEASE_BIN" ]] && supports_guest_exec_cli "$FALLBACK_RELEASE_BIN"; then
		QARAX_BIN="$FALLBACK_RELEASE_BIN"
	else
		die "No qarax CLI with guest exec support found. Rebuild the CLI or run REBUILD=1 ./hack/run-local.sh"
	fi
fi

QARAX=("$QARAX_BIN" "--server" "$SERVER")

ensure_stack "$SERVER"

cleanup() {
	if [[ "$KEEP" -eq 1 ]]; then
		echo
		info "Keeping demo resources:"
		info "  VM: $VM_NAME"
		info "  Template: $TEMPLATE_NAME"
		info "  Boot source: $BOOT_SOURCE_NAME"
		info "  Pool: $POOL_NAME"
		return 0
	fi

	echo
	step "Cleaning up demo resources..."
	# Delete tears the VM down on the node even if it is still running; the
	# force-stop is best effort. Avoid `--wait`: it has no timeout.
	"${QARAX[@]}" vm force-stop "$VM_NAME" >/dev/null 2>&1 || true
	"${QARAX[@]}" vm delete "$VM_NAME" >/dev/null 2>&1 || true
	"${QARAX[@]}" vm-template delete "$TEMPLATE_NAME" >/dev/null 2>&1 || true
	"${QARAX[@]}" boot-source delete "$BOOT_SOURCE_NAME" >/dev/null 2>&1 || true
	"${QARAX[@]}" storage-object delete "$KERNEL_NAME" >/dev/null 2>&1 || true
	"${QARAX[@]}" storage-object delete "$INITRAMFS_NAME" >/dev/null 2>&1 || true
	[[ -n "$HOST_NAME" ]] && "${QARAX[@]}" storage-pool detach-host "$POOL_NAME" "$HOST_NAME" >/dev/null 2>&1 || true
	"${QARAX[@]}" storage-pool delete "$POOL_NAME" >/dev/null 2>&1 || true
	info "Done."
}
trap cleanup EXIT

wait_for_vm_status() {
	local vm_name="$1"
	local expected="$2"
	local timeout_secs="$3"
	local deadline=$((SECONDS + timeout_secs))
	local status=""

	while ((SECONDS < deadline)); do
		status=$("${QARAX[@]}" -o json vm get "$vm_name" | jq -r '.status') || true
		if [[ "$status" == "$expected" ]]; then
			return 0
		fi
		sleep 2
	done

	"${QARAX[@]}" vm get "$vm_name" || true
	die "Timed out waiting for VM '$vm_name' to reach status '$expected' (last status: ${status:-unknown})"
}

run_guest_exec() {
	local vm_name="$1"
	local deadline=$((SECONDS + EXEC_READY_TIMEOUT))
	local attempt=1
	local output=""
	local exit_code=""
	local timed_out=""
	local stdout=""
	local stderr=""
	local last_error=""

	echo -e "  ${DIM}\$ ${QARAX_BIN} --server ${SERVER} -o json vm exec ${vm_name} --timeout ${EXEC_TIMEOUT} -- /bin/sh -c '${EXEC_PAYLOAD}'${NC}"
	while ((SECONDS < deadline)); do
		if output=$("${QARAX[@]}" -o json vm exec "$vm_name" --timeout "$EXEC_TIMEOUT" -- /bin/sh -c "$EXEC_PAYLOAD" 2>&1); then
			exit_code=$(jq -r '.exit_code' <<<"$output")
			timed_out=$(jq -r '.timed_out' <<<"$output")
			stdout=$(jq -r '.stdout' <<<"$output")
			stderr=$(jq -r '.stderr' <<<"$output")

			[[ "$timed_out" == "false" ]] || die "Guest exec timed out"
			[[ "$exit_code" == "0" ]] || die "Guest exec returned exit code $exit_code"
			[[ "$stdout" == *"vm-exec"* ]] || die "Guest exec output missing marker: $stdout"
			[[ "$stdout" == *"Linux"* ]] || die "Guest exec output missing uname: $stdout"

			EXEC_STDOUT="$stdout"
			EXEC_STDERR="$stderr"
			return 0
		fi

		last_error="$output"
		info "Guest agent not ready yet (attempt ${attempt}); retrying in 2s..."
		sleep 2
		attempt=$((attempt + 1))
	done

	echo "$last_error" >&2
	die "Timed out waiting for guest exec to become ready"
}

banner "Regular VM Guest Exec Demo"

step "Using qarax CLI:"
info "$QARAX_BIN"

if [[ -z "$HOST_NAME" ]]; then
	HOST_NAME=$("${QARAX[@]}" -o json host list | jq -r '[.[] | select(.status == "up")] | .[0].name // empty')
fi
[[ -n "$HOST_NAME" ]] || die "No UP host found"

step "Using host '$HOST_NAME' and built-in guest-agent initramfs..."
info "Kernel: $KERNEL_PATH"
info "Initramfs: $INITRAMFS_PATH"
info "VM: $VM_NAME"

step "Creating a local storage pool for boot artifacts..."
run "${QARAX[@]}" storage-pool create \
	--name "$POOL_NAME" \
	--pool-type local \
	--config "{\"path\":\"$POOL_PATH\"}" \
	--host "$HOST_NAME"
echo

step "Transferring the demo kernel and guest-agent initramfs..."
run "${QARAX[@]}" transfer create \
	--pool "$POOL_NAME" \
	--name "$KERNEL_NAME" \
	--source "$KERNEL_PATH" \
	--object-type kernel \
	--wait
run "${QARAX[@]}" transfer create \
	--pool "$POOL_NAME" \
	--name "$INITRAMFS_NAME" \
	--source "$INITRAMFS_PATH" \
	--object-type initrd \
	--wait
echo

step "Creating a boot source and reusable VM template..."
run "${QARAX[@]}" boot-source create \
	--name "$BOOT_SOURCE_NAME" \
	--kernel "$KERNEL_NAME" \
	--initrd "$INITRAMFS_NAME" \
	--params "console=ttyS0"
run "${QARAX[@]}" vm-template create \
	--name "$TEMPLATE_NAME" \
	--hypervisor cloud_hv \
	--boot-source "$BOOT_SOURCE_NAME" \
	--vcpus "$VCPUS" \
	--memory "$MEMORY_BYTES" \
	--boot-mode kernel
echo

step "Creating a regular VM with guest exec enabled..."
run "${QARAX[@]}" vm create \
	--name "$VM_NAME" \
	--template "$TEMPLATE_NAME" \
	--guest-agent
echo

step "Starting the VM and waiting for it to reach running..."
run "${QARAX[@]}" vm start "$VM_NAME"
wait_for_vm_status "$VM_NAME" "running" 120
run "${QARAX[@]}" vm get "$VM_NAME"
echo

step "Running a command inside the guest via qarax vm exec..."
run_guest_exec "$VM_NAME"
info "Guest command stdout:"
printf '%s\n' "$EXEC_STDOUT"
if [[ -n "${EXEC_STDERR:-}" ]]; then
	info "Guest command stderr:"
	printf '%s\n' "$EXEC_STDERR"
fi
echo

banner "Demo Complete"
info "This demo created a regular VM from a bootable template, enabled the guest agent,"
info "then executed '/bin/sh -c \"$EXEC_PAYLOAD\"' inside the running guest."
