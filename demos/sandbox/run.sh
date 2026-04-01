#!/usr/bin/env bash
#
# Demo: qarax sandbox feature
#
# Sandboxes are ephemeral VMs spun up from a VM template and automatically
# reaped after an idle timeout.  This demo shows the full lifecycle:
#
#   1. Create (or reuse) a VM template backed by a boot source with initramfs
#   2. Create a sandbox from the template
#   3. Poll until the sandbox is ready
#   4. Execute a command inside the sandbox
#   5. Inspect the sandbox (status, IP, idle timeout)
#   6. Create a second sandbox — demonstrating rapid provisioning from the same template
#   7. Delete one sandbox manually
#   8. Watch the remaining sandbox auto-expire after its short idle timeout
#
# Each sandbox creates its own underlying VM automatically; no manual VM
# lifecycle management is required.
#
# Prerequisites:
#   - qarax stack running (./hack/run-local.sh)
#   - qarax CLI on PATH
#
# Usage:
#   ./demos/sandbox/run.sh
#   ./demos/sandbox/run.sh --server http://localhost:8000
#   ./demos/sandbox/run.sh --template my-template     # reuse an existing template
#   ./demos/sandbox/run.sh --idle-timeout 60          # custom idle timeout in seconds
#   SANDBOX_INITRAMFS_PATH=/path/to/initramfs ./demos/sandbox/run.sh
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
IDLE_TIMEOUT="${SANDBOX_IDLE_TIMEOUT:-90}"
TEMPLATE_NAME="sandbox-demo-template"
BOOT_SOURCE_NAME="sandbox-demo-boot"
HOST_NAME="${QARAX_HOST:-local-node}"
POOL_NAME="sandbox-demo-pool"
POOL_PATH="/var/lib/qarax/images"
KERNEL_PATH="/var/lib/qarax/images/vmlinux"
KERNEL_NAME="sandbox-demo-kernel"
INITRAMFS_PATH="${SANDBOX_INITRAMFS_PATH:-/var/lib/qarax/images/test-initramfs.gz}"
INITRAMFS_NAME="sandbox-demo-initramfs"
SANDBOX_NAME_PREFIX="sandbox-demo"
SANDBOX1_NAME=""
SANDBOX2_NAME=""
DEFAULT_TEMPLATE_NAME="$TEMPLATE_NAME"
MANAGED_TEMPLATE_PREFIX="sandbox-demo-template"
MANAGED_BOOT_SOURCE_PREFIX="sandbox-demo-boot"
MANAGED_KERNEL_PREFIX="sandbox-demo-kernel"
MANAGED_INITRAMFS_PREFIX="sandbox-demo-initramfs"

SANDBOX1_ID=""
SANDBOX2_ID=""
TEMPLATE_ID=""
TEMPLATE_CREATED=0
BOOT_SOURCE_CREATED=0
KERNEL_CREATED=0
INITRAMFS_CREATED=0
MANAGE_TEMPLATE_ASSETS=1
CLEANUP_ONLY=0
RUN_SUFFIX="$(date +%s)-$$"

TEMPLATE_NAME="${MANAGED_TEMPLATE_PREFIX}-${RUN_SUFFIX}"
BOOT_SOURCE_NAME="${MANAGED_BOOT_SOURCE_PREFIX}-${RUN_SUFFIX}"
KERNEL_NAME="${MANAGED_KERNEL_PREFIX}-${RUN_SUFFIX}"
INITRAMFS_NAME="${MANAGED_INITRAMFS_PREFIX}-${RUN_SUFFIX}"
SANDBOX1_NAME="${SANDBOX_NAME_PREFIX}-1-${RUN_SUFFIX}"
SANDBOX2_NAME="${SANDBOX_NAME_PREFIX}-2-${RUN_SUFFIX}"

# Parse arguments
while [[ $# -gt 0 ]]; do
	case $1 in
	--server)
		SERVER="$2"
		shift 2
		;;
	--template)
		TEMPLATE_NAME="$2"
		MANAGE_TEMPLATE_ASSETS=0
		shift 2
		;;
	--idle-timeout)
		IDLE_TIMEOUT="$2"
		shift 2
		;;
	--initramfs)
		INITRAMFS_PATH="$2"
		shift 2
		;;
	--cleanup)
		CLEANUP_ONLY=1
		shift 1
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo ""
		echo "Options:"
		echo "  --server URL          qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --template NAME       VM template name to use or create (default: sandbox-demo-template)"
		echo "  --idle-timeout SECS   Idle timeout for sandboxes in seconds (default: 90)"
		echo "  --initramfs PATH      Initramfs path on the host running qarax-node"
		echo "  --cleanup             Remove demo-managed sandboxes, VMs, and template assets, then exit"
		exit 0
		;;
	*)
		echo "Unknown option: $1"
		exit 1
		;;
	esac
done

[[ -n "$INITRAMFS_PATH" ]] || die "sandbox demo requires an initramfs with qarax-init; set SANDBOX_INITRAMFS_PATH or pass --initramfs"

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

if [[ -z "${SKIP_BUILD:-}" ]]; then
	step "Building qarax CLI..."
	cargo build -p cli
fi

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"
QARAX="$QARAX_BIN --server $SERVER"

cleanup() {
	echo
	step "Cleaning up..."
	if [[ -n "$SANDBOX1_ID" ]]; then
		$QARAX sandbox delete "$SANDBOX1_ID" 2>/dev/null || true
	fi
	if [[ -n "$SANDBOX2_ID" ]]; then
		$QARAX sandbox delete "$SANDBOX2_ID" 2>/dev/null || true
	fi
	if [[ "$TEMPLATE_CREATED" -eq 1 ]]; then
		$QARAX vm-template delete "$TEMPLATE_NAME" 2>/dev/null || true
	fi
	if [[ "$BOOT_SOURCE_CREATED" -eq 1 ]]; then
		$QARAX boot-source delete "$BOOT_SOURCE_NAME" 2>/dev/null || true
	fi
	if [[ "$INITRAMFS_CREATED" -eq 1 ]]; then
		$QARAX storage-object delete "$INITRAMFS_NAME" 2>/dev/null || true
	fi
	if [[ "$KERNEL_CREATED" -eq 1 ]]; then
		$QARAX storage-object delete "$KERNEL_NAME" 2>/dev/null || true
	fi
	info "Done."
}
trap cleanup EXIT

json_field() {
	local field="$1"
	python3 -c 'import json, sys; print(json.load(sys.stdin)[sys.argv[1]])' "$field"
}

list_names_with_prefix() {
	local kind="$1"
	local prefix="$2"
	$QARAX "$kind" list -o json 2>/dev/null | python3 -c '
import json, sys
prefix = sys.argv[1]
for item in json.load(sys.stdin):
    name = item.get("name") or ""
    if name.startswith(prefix):
        print(name)
' "$prefix" 2>/dev/null || true
}

list_ids_with_prefix() {
	local kind="$1"
	local prefix="$2"
	$QARAX "$kind" list -o json 2>/dev/null | python3 -c '
import json, sys
prefix = sys.argv[1]
for item in json.load(sys.stdin):
    name = item.get("name") or ""
    if name.startswith(prefix):
        print(item["id"])
' "$prefix" 2>/dev/null || true
}

sandbox_status() {
	local sandbox_id="$1"
	$QARAX sandbox list -o json 2>/dev/null | python3 -c '
import json, sys
target = sys.argv[1]
for sandbox in json.load(sys.stdin):
    if sandbox.get("id") == target:
        print(sandbox.get("status", "unknown"))
        break
else:
    print("gone")
' "$sandbox_id" 2>/dev/null || echo "gone"
}

delete_matching_names() {
	local kind="$1"
	local prefix="$2"
	local names
	names=$(list_names_with_prefix "$kind" "$prefix")
	for name in $names; do
		$QARAX "$kind" delete "$name" >/dev/null 2>&1 || true
	done
}

wait_for_no_matching_ids() {
	local kind="$1"
	local prefix="$2"
	local timeout="$3"
	local elapsed=0
	local ids
	while ((elapsed < timeout)); do
		ids=$(list_ids_with_prefix "$kind" "$prefix")
		[[ -z "$ids" ]] && return 0
		sleep 2
		elapsed=$((elapsed + 2))
	done
	return 1
}

wait_for_sandbox_status() {
	local sandbox_id="$1"
	local target="$2"
	local status
	while true; do
		status=$(sandbox_status "$sandbox_id")
		case "$status" in
		"$target")
			echo ""
			return 0
			;;
		error)
			echo ""
			$QARAX sandbox get "$sandbox_id" >/dev/null 2>&1 || true
			return 1
			;;
		esac
		echo -ne "\r[${status}]   "
		sleep 2
	done
}

ensure_clean_demo_state() {
	local sandbox_id
	for sandbox_id in $(list_ids_with_prefix sandbox "$SANDBOX_NAME_PREFIX-"); do
		$QARAX sandbox delete "$sandbox_id" >/dev/null 2>&1 || true
	done
	for vm_id in $(list_ids_with_prefix vm "$SANDBOX_NAME_PREFIX-"); do
		$QARAX vm delete "$vm_id" >/dev/null 2>&1 || true
	done
	if ! wait_for_no_matching_ids sandbox "$SANDBOX_NAME_PREFIX-" 30; then
		info "Some prior demo sandboxes are still terminating; continuing with fresh run suffixes."
	fi
	if ! wait_for_no_matching_ids vm "$SANDBOX_NAME_PREFIX-" 30; then
		info "Some prior demo VMs are still terminating; continuing with fresh run suffixes."
	fi
	if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
		delete_matching_names vm-template "$MANAGED_TEMPLATE_PREFIX-"
		delete_matching_names boot-source "$MANAGED_BOOT_SOURCE_PREFIX-"
		delete_matching_names storage-object "$MANAGED_KERNEL_PREFIX-"
		delete_matching_names storage-object "$MANAGED_INITRAMFS_PREFIX-"
	fi
}

cleanup_demo_resources() {
	step "Cleaning prior demo resources..."
	ensure_clean_demo_state
	info "Demo resources removed (if any)."
}

create_sandbox() {
	local name="$1"
	local output
	if ! output=$("$QARAX_BIN" --server "$SERVER" sandbox create \
		--template "$TEMPLATE_NAME" \
		--name "$name" \
		--idle-timeout "$IDLE_TIMEOUT" -o json); then
		return 1
	fi
	printf '  %s\n' "$output" >&2
	printf '%s' "$output" | json_field "id"
}

banner "Sandbox Demo"

step "Checking qarax API..."
$QARAX host list -o json >/dev/null || die "Cannot reach qarax at $SERVER"
info "API reachable at $SERVER"

if [[ "$CLEANUP_ONLY" -eq 1 ]]; then
	banner "Cleanup"
	cleanup_demo_resources
	trap - EXIT
	exit 0
fi

if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	info "Using demo-managed template assets rooted at '$DEFAULT_TEMPLATE_NAME'."
else
	info "Using caller-managed template '$TEMPLATE_NAME'."
fi

banner "Step 1 — VM Template"

cleanup_demo_resources
echo ""

if $QARAX vm-template get "$TEMPLATE_NAME" >/dev/null 2>&1; then
	info "Template '$TEMPLATE_NAME' already exists — reusing it."
	TEMPLATE_ID=$($QARAX vm-template get "$TEMPLATE_NAME" -o json | json_field "id" 2>/dev/null)
else
	step "Creating storage pool and boot source for the template..."

	if ! $QARAX storage-pool get "$POOL_NAME" >/dev/null 2>&1; then
		run $QARAX storage-pool create \
			--name "$POOL_NAME" \
			--pool-type local \
			--config "{\"path\":\"$POOL_PATH\"}" \
			--host "$HOST_NAME"
		echo ""
	else
		info "Storage pool '$POOL_NAME' already exists."
		run $QARAX storage-pool attach-host "$POOL_NAME" --all
	fi

	if ! $QARAX storage-object get "$KERNEL_NAME" >/dev/null 2>&1; then
		run $QARAX transfer create \
			--pool "$POOL_NAME" \
			--name "$KERNEL_NAME" \
			--source "$KERNEL_PATH" \
			--object-type kernel \
			--wait
		KERNEL_CREATED=1
		echo ""
	else
		info "Kernel object '$KERNEL_NAME' already exists."
	fi

	INITRAMFS_FLAG=""
	if [[ -n "$INITRAMFS_PATH" ]]; then
		if ! $QARAX storage-object get "$INITRAMFS_NAME" >/dev/null 2>&1; then
			run $QARAX transfer create \
				--pool "$POOL_NAME" \
				--name "$INITRAMFS_NAME" \
				--source "$INITRAMFS_PATH" \
				--object-type initrd \
				--wait
			INITRAMFS_CREATED=1
			echo ""
		else
			info "Initramfs object '$INITRAMFS_NAME' already exists."
		fi
		INITRAMFS_FLAG="--initrd $INITRAMFS_NAME"
	fi

	if ! $QARAX boot-source get "$BOOT_SOURCE_NAME" >/dev/null 2>&1; then
		run $QARAX boot-source create \
			--name "$BOOT_SOURCE_NAME" \
			--kernel "$KERNEL_NAME" \
			$INITRAMFS_FLAG \
			--params "console=ttyS0"
		BOOT_SOURCE_CREATED=1
		echo ""
	else
		info "Boot source '$BOOT_SOURCE_NAME' already exists."
	fi

	step "Creating VM template '$TEMPLATE_NAME'..."
	echo ""
	run $QARAX vm-template create \
		--name "$TEMPLATE_NAME" \
		--boot-source "$BOOT_SOURCE_NAME" \
		--vcpus 1 \
		--memory 268435456 \
		--boot-mode kernel
	echo ""

	TEMPLATE_ID=$($QARAX vm-template get "$TEMPLATE_NAME" -o json | json_field "id" 2>/dev/null)
	TEMPLATE_CREATED=1
	info "Template '$TEMPLATE_NAME' created (id=${TEMPLATE_ID})"
fi

banner "Step 2 — Create Sandbox"

step "Creating sandbox from template '$TEMPLATE_NAME' (idle timeout: ${IDLE_TIMEOUT}s)..."
echo ""
SANDBOX1_ID=$(create_sandbox "$SANDBOX1_NAME")
wait_for_sandbox_status "$SANDBOX1_ID" ready || die "Sandbox $SANDBOX1_ID failed to become ready"
echo ""

[[ -n "$SANDBOX1_ID" ]] || die "Could not determine sandbox ID from list"
info "Sandbox 1 ID: $SANDBOX1_ID"
echo ""

step "Sandbox 1 details:"
run $QARAX sandbox get "$SANDBOX1_ID"
echo ""

banner "Step 3 — Execute Inside Sandbox"

step "Running a command inside sandbox #1..."
echo ""
run $QARAX sandbox exec "$SANDBOX1_ID" -- /bin/sh -c 'printf sandbox-demo && uname -s'
echo ""

banner "Step 4 — Rapid Provisioning (second sandbox)"

info "Demonstrating that multiple sandboxes can be provisioned from the same template concurrently."
echo ""

step "Creating sandbox #2..."
echo ""
SANDBOX2_ID=$(create_sandbox "$SANDBOX2_NAME")
wait_for_sandbox_status "$SANDBOX2_ID" ready || die "Sandbox $SANDBOX2_ID failed to become ready"
echo ""

[[ -n "$SANDBOX2_ID" ]] || die "Could not determine sandbox #2 ID from list"
info "Sandbox 2 ID: $SANDBOX2_ID"
echo ""

step "All sandboxes:"
run $QARAX sandbox list
echo ""

banner "Step 5 — Manual Delete"

step "Deleting sandbox #1 (${SANDBOX1_ID::8}...) manually..."
echo ""
run $QARAX sandbox delete "$SANDBOX1_ID"
SANDBOX1_ID=""
echo ""

step "Remaining sandboxes:"
run $QARAX sandbox list
echo ""

banner "Step 6 — Auto-Reap via Idle Timeout"

info "Sandbox #2 has an idle timeout of ${IDLE_TIMEOUT}s."
info "The sandbox reaper checks every 15s and will destroy it once the timeout expires."
info "Waiting for sandbox #2 to disappear (up to $((IDLE_TIMEOUT + 60))s)..."
echo ""

elapsed=0
max_wait=$((IDLE_TIMEOUT + 60))
while true; do
	status=$(sandbox_status "$SANDBOX2_ID")
	case "$status" in
	gone | destroying)
		echo -e "  ${GREEN}✓ Sandbox reaped (${elapsed}s)${NC}"
		SANDBOX2_ID=""
		break
		;;
	esac
	[[ $elapsed -ge $max_wait ]] && {
		echo -e "  ${YELLOW}⚠ Sandbox still alive after ${max_wait}s — leaving cleanup to the trap${NC}"
		break
	}
	sleep 5
	elapsed=$((elapsed + 5))
	echo -ne "  \r  ${DIM}${status} … ${elapsed}s / ${max_wait}s${NC}   "
done
echo ""

banner "Demo Complete"

echo -e "${GREEN}What we demonstrated:${NC}"
echo "  ✓ Create a VM template from a boot source"
echo "  ✓ Spin up an ephemeral sandbox from that template"
echo "  ✓ Poll the sandbox until it is ready"
echo "  ✓ Execute a command inside the sandbox over the guest agent"
echo "  ✓ Provision a second sandbox concurrently from the same template"
echo "  ✓ Delete a sandbox manually"
echo "  ✓ Watch idle-timeout auto-reap kick in"
echo ""
echo "Useful commands:"
echo "  qarax sandbox list                 # list all sandboxes"
echo "  qarax sandbox get <id>             # inspect a sandbox"
echo "  qarax sandbox create --template T  # create a new sandbox"
echo "  qarax sandbox exec <id> -- cmd     # run a command inside a sandbox"
echo "  qarax sandbox delete <id>          # delete a sandbox immediately"
echo "  qarax vm-template list             # list all VM templates"
