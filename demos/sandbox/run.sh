#!/usr/bin/env bash
#
# Demo: qarax sandbox feature
#
# Sandboxes are ephemeral VMs spun up from a VM template and automatically
# reaped after an idle timeout.  This demo shows the full lifecycle:
#
#   1. Create Firecracker + Cloud Hypervisor templates backed by one boot source
#   2. Create one sandbox from each template and measure time-to-ready
#   3. Inspect the Firecracker sandbox (default managed backend)
#   4. Execute a command inside the Firecracker sandbox
#   5. Configure a prewarmed sandbox pool for the Firecracker template
#   6. Claim a second Firecracker sandbox from the prewarmed pool
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
POOL_MIN_READY="${SANDBOX_POOL_MIN_READY:-1}"
TEMPLATE_NAME="sandbox-demo-template"
CLOUDHV_TEMPLATE_NAME="sandbox-demo-template-cloudhv"
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
CLOUDHV_SANDBOX_NAME=""
DEFAULT_TEMPLATE_NAME="$TEMPLATE_NAME"
DEFAULT_CLOUDHV_TEMPLATE_NAME="$CLOUDHV_TEMPLATE_NAME"
MANAGED_TEMPLATE_PREFIX="sandbox-demo-template"
MANAGED_CLOUDHV_TEMPLATE_PREFIX="sandbox-demo-template-cloudhv"
MANAGED_BOOT_SOURCE_PREFIX="sandbox-demo-boot"
MANAGED_KERNEL_PREFIX="sandbox-demo-kernel"
MANAGED_INITRAMFS_PREFIX="sandbox-demo-initramfs"

SANDBOX1_ID=""
SANDBOX2_ID=""
CLOUDHV_SANDBOX_ID=""
TEMPLATE_ID=""
CLOUDHV_TEMPLATE_ID=""
TEMPLATE_CREATED=0
CLOUDHV_TEMPLATE_CREATED=0
BOOT_SOURCE_CREATED=0
KERNEL_CREATED=0
INITRAMFS_CREATED=0
MANAGE_TEMPLATE_ASSETS=1
CLEANUP_ONLY=0
FC_READY_MS=""
CH_READY_MS=""
WARM_READY_MS=""
RUN_SUFFIX="$(date +%s)-$$"

TEMPLATE_NAME="${MANAGED_TEMPLATE_PREFIX}-${RUN_SUFFIX}"
CLOUDHV_TEMPLATE_NAME="${MANAGED_CLOUDHV_TEMPLATE_PREFIX}-${RUN_SUFFIX}"
BOOT_SOURCE_NAME="${MANAGED_BOOT_SOURCE_PREFIX}-${RUN_SUFFIX}"
KERNEL_NAME="${MANAGED_KERNEL_PREFIX}-${RUN_SUFFIX}"
INITRAMFS_NAME="${MANAGED_INITRAMFS_PREFIX}-${RUN_SUFFIX}"
SANDBOX1_NAME="${SANDBOX_NAME_PREFIX}-1-${RUN_SUFFIX}"
SANDBOX2_NAME="${SANDBOX_NAME_PREFIX}-2-${RUN_SUFFIX}"
CLOUDHV_SANDBOX_NAME="${SANDBOX_NAME_PREFIX}-ch-${RUN_SUFFIX}"

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

if [[ "$CLEANUP_ONLY" -eq 0 ]]; then
	ensure_stack "$SERVER"
fi

cleanup() {
	echo
	step "Cleaning up..."
	if [[ -n "$SANDBOX1_ID" ]]; then
		$QARAX sandbox delete "$SANDBOX1_ID" 2>/dev/null || true
	fi
	if [[ -n "$SANDBOX2_ID" ]]; then
		$QARAX sandbox delete "$SANDBOX2_ID" 2>/dev/null || true
	fi
	if [[ -n "$CLOUDHV_SANDBOX_ID" ]]; then
		$QARAX sandbox delete "$CLOUDHV_SANDBOX_ID" 2>/dev/null || true
	fi
	$QARAX sandbox pool delete --template "$TEMPLATE_NAME" 2>/dev/null || true
	$QARAX sandbox pool delete --template "$CLOUDHV_TEMPLATE_NAME" 2>/dev/null || true
	if [[ "$TEMPLATE_CREATED" -eq 1 ]]; then
		$QARAX vm-template delete "$TEMPLATE_NAME" 2>/dev/null || true
	fi
	if [[ "$CLOUDHV_TEMPLATE_CREATED" -eq 1 ]]; then
		$QARAX vm-template delete "$CLOUDHV_TEMPLATE_NAME" 2>/dev/null || true
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

sandbox_pool_ready_count() {
	local template_name="$1"
	$QARAX sandbox pool get --template "$template_name" -o json 2>/dev/null | python3 -c '
import json, sys
data = json.load(sys.stdin)
print(data.get("current_ready", 0))
' 2>/dev/null || echo "0"
}

wait_for_sandbox_pool_ready() {
	local template_name="$1"
	local target_ready="$2"
	local timeout="$3"
	local elapsed=0
	local ready=0
	while ((elapsed < timeout)); do
		ready=$(sandbox_pool_ready_count "$template_name")
		if ((ready >= target_ready)); then
			echo ""
			return 0
		fi
		echo -ne "\r[pool ready=${ready}/${target_ready}]   "
		sleep 2
		elapsed=$((elapsed + 2))
	done
	echo ""
	return 1
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
		delete_matching_names vm-template "$MANAGED_CLOUDHV_TEMPLATE_PREFIX-"
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

now_ms() {
	python3 -c 'import time; print(time.perf_counter_ns() // 1_000_000)'
}

create_sandbox() {
	local template_name="$1"
	local name="$2"
	local output
	if ! output=$("$QARAX_BIN" --server "$SERVER" sandbox create \
		--template "$template_name" \
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

banner "Step 1 — VM Templates"

cleanup_demo_resources
echo ""

if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	step "Creating storage pool and boot source for the sandbox templates..."

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

	step "Creating Firecracker default sandbox template '$TEMPLATE_NAME'..."
	echo ""
	run $QARAX vm-template create \
		--name "$TEMPLATE_NAME" \
		--hypervisor firecracker \
		--boot-source "$BOOT_SOURCE_NAME" \
		--vcpus 1 \
		--memory 268435456 \
		--boot-mode kernel
	echo ""
	TEMPLATE_ID=$($QARAX vm-template get "$TEMPLATE_NAME" -o json | json_field "id" 2>/dev/null)
	TEMPLATE_CREATED=1
	info "Firecracker template '$TEMPLATE_NAME' created (id=${TEMPLATE_ID})"

	step "Creating Cloud Hypervisor comparison template '$CLOUDHV_TEMPLATE_NAME'..."
	echo ""
	run $QARAX vm-template create \
		--name "$CLOUDHV_TEMPLATE_NAME" \
		--hypervisor cloud_hv \
		--boot-source "$BOOT_SOURCE_NAME" \
		--vcpus 1 \
		--memory 268435456 \
		--boot-mode kernel
	echo ""
	CLOUDHV_TEMPLATE_ID=$($QARAX vm-template get "$CLOUDHV_TEMPLATE_NAME" -o json | json_field "id" 2>/dev/null)
	CLOUDHV_TEMPLATE_CREATED=1
	info "Cloud Hypervisor comparison template '$CLOUDHV_TEMPLATE_NAME' created (id=${CLOUDHV_TEMPLATE_ID})"
else
	$QARAX vm-template get "$TEMPLATE_NAME" >/dev/null 2>&1 || die "Template '$TEMPLATE_NAME' not found"
	TEMPLATE_ID=$($QARAX vm-template get "$TEMPLATE_NAME" -o json | json_field "id" 2>/dev/null)
	info "Using caller-managed template '$TEMPLATE_NAME' (id=${TEMPLATE_ID})."
fi

if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	banner "Step 2 — Provisioning Benchmark"
	info "Benchmarking time-to-ready with identical guest artifacts."
	echo ""

	step "Creating Firecracker sandbox #1 (default backend)..."
	echo ""
	start_ms=$(now_ms)
	SANDBOX1_ID=$(create_sandbox "$TEMPLATE_NAME" "$SANDBOX1_NAME")
	wait_for_sandbox_status "$SANDBOX1_ID" ready || die "Sandbox $SANDBOX1_ID failed to become ready"
	FC_READY_MS=$(($(now_ms) - start_ms))
	echo ""
	info "Firecracker sandbox ready in ${FC_READY_MS} ms"
	echo ""

	step "Creating Cloud Hypervisor comparison sandbox..."
	echo ""
	start_ms=$(now_ms)
	CLOUDHV_SANDBOX_ID=$(create_sandbox "$CLOUDHV_TEMPLATE_NAME" "$CLOUDHV_SANDBOX_NAME")
	wait_for_sandbox_status "$CLOUDHV_SANDBOX_ID" ready || die "Sandbox $CLOUDHV_SANDBOX_ID failed to become ready"
	CH_READY_MS=$(($(now_ms) - start_ms))
	echo ""
	info "Cloud Hypervisor sandbox ready in ${CH_READY_MS} ms"
	echo ""

	if ((FC_READY_MS < CH_READY_MS)); then
		info "In this run, Firecracker reached READY $((CH_READY_MS - FC_READY_MS)) ms faster than Cloud Hypervisor."
	elif ((CH_READY_MS < FC_READY_MS)); then
		info "In this run, Cloud Hypervisor reached READY $((FC_READY_MS - CH_READY_MS)) ms faster than Firecracker."
	else
		info "In this run, Firecracker and Cloud Hypervisor reached READY in the same time."
	fi

	banner "Step 3 — Configure Prewarmed Sandbox Pool"
	step "Keeping ${POOL_MIN_READY} Firecracker sandbox(s) ready for instant claims..."
	echo ""
	run $QARAX sandbox pool set --template "$TEMPLATE_NAME" --min-ready "$POOL_MIN_READY"
	wait_for_sandbox_pool_ready "$TEMPLATE_NAME" "$POOL_MIN_READY" 120 || die "Sandbox pool for '$TEMPLATE_NAME' did not become ready"
	echo ""
	step "Current sandbox pool status:"
	run $QARAX sandbox pool get --template "$TEMPLATE_NAME"
	echo ""

	banner "Step 4 — Inspect Firecracker Sandbox"
else
	banner "Step 2 — Create Sandbox"
	step "Creating sandbox from template '$TEMPLATE_NAME' (idle timeout: ${IDLE_TIMEOUT}s)..."
	echo ""
	SANDBOX1_ID=$(create_sandbox "$TEMPLATE_NAME" "$SANDBOX1_NAME")
	wait_for_sandbox_status "$SANDBOX1_ID" ready || die "Sandbox $SANDBOX1_ID failed to become ready"
	echo ""

	banner "Step 3 — Configure Prewarmed Sandbox Pool"
	step "Keeping ${POOL_MIN_READY} sandbox(s) ready for instant claims..."
	echo ""
	run $QARAX sandbox pool set --template "$TEMPLATE_NAME" --min-ready "$POOL_MIN_READY"
	wait_for_sandbox_pool_ready "$TEMPLATE_NAME" "$POOL_MIN_READY" 120 || die "Sandbox pool for '$TEMPLATE_NAME' did not become ready"
	echo ""
fi

[[ -n "$SANDBOX1_ID" ]] || die "Could not determine sandbox ID from list"
info "Sandbox 1 ID: $SANDBOX1_ID"
echo ""

step "Sandbox 1 details:"
run $QARAX sandbox get "$SANDBOX1_ID"
echo ""

if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	banner "Step 5 — Execute Inside Firecracker Sandbox"
	step "Running a command inside sandbox #1..."
	echo ""
	run $QARAX sandbox exec "$SANDBOX1_ID" -- /bin/sh -c 'printf sandbox-demo && uname -s'
	echo ""

	step "Deleting the Cloud Hypervisor comparison sandbox..."
	echo ""
	run $QARAX sandbox delete "$CLOUDHV_SANDBOX_ID"
	CLOUDHV_SANDBOX_ID=""
	echo ""

	banner "Step 6 — Claim a Prewarmed Firecracker Sandbox"
else
	banner "Step 4 — Execute Inside Sandbox"
	step "Running a command inside sandbox #1..."
	echo ""
	run $QARAX sandbox exec "$SANDBOX1_ID" -- /bin/sh -c 'printf sandbox-demo && uname -s'
	echo ""

	banner "Step 5 — Claim a Prewarmed Sandbox"
fi

info "The second sandbox should claim the already-running standby VM from the pool."
echo ""

step "Creating sandbox #2 from the prewarmed pool..."
echo ""
start_ms=$(now_ms)
SANDBOX2_ID=$(create_sandbox "$TEMPLATE_NAME" "$SANDBOX2_NAME")
wait_for_sandbox_status "$SANDBOX2_ID" ready || die "Sandbox $SANDBOX2_ID failed to become ready"
WARM_READY_MS=$(($(now_ms) - start_ms))
echo ""
info "Prewarmed sandbox ready in ${WARM_READY_MS} ms"
echo ""

[[ -n "$SANDBOX2_ID" ]] || die "Could not determine sandbox #2 ID from list"
info "Sandbox 2 ID: $SANDBOX2_ID"
echo ""

step "Sandbox pool after the claim:"
run $QARAX sandbox pool get --template "$TEMPLATE_NAME"
echo ""

step "All sandboxes:"
run $QARAX sandbox list
echo ""

if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	banner "Step 7 — Manual Delete"
else
	banner "Step 6 — Manual Delete"
fi

step "Deleting sandbox #1 (${SANDBOX1_ID::8}...) manually..."
echo ""
run $QARAX sandbox delete "$SANDBOX1_ID"
SANDBOX1_ID=""
echo ""

step "Remaining sandboxes:"
run $QARAX sandbox list
echo ""

if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	banner "Step 8 — Auto-Reap via Idle Timeout"
else
	banner "Step 7 — Auto-Reap via Idle Timeout"
fi

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
echo "  ✓ Create sandbox templates from a shared boot source"
echo "  ✓ Default managed sandboxes to Firecracker"
echo "  ✓ Benchmark Firecracker vs Cloud Hypervisor time-to-ready"
echo "  ✓ Prewarm a Firecracker sandbox pool"
echo "  ✓ Inspect a running Firecracker sandbox"
echo "  ✓ Execute a command inside the sandbox over the guest agent"
echo "  ✓ Claim a second sandbox from the prewarmed pool"
echo "  ✓ Delete a sandbox manually"
echo "  ✓ Watch idle-timeout auto-reap kick in"
echo ""
if [[ -n "$FC_READY_MS" && -n "$CH_READY_MS" ]]; then
	echo "Measured performance in this run:"
	echo "  Firecracker ready time:      ${FC_READY_MS} ms"
	echo "  Cloud Hypervisor ready time: ${CH_READY_MS} ms"
	if [[ -n "$WARM_READY_MS" ]]; then
		echo "  Prewarmed claim ready time:  ${WARM_READY_MS} ms"
	fi
	echo ""
elif [[ -n "$WARM_READY_MS" ]]; then
	echo "Measured performance in this run:"
	echo "  Cold sandbox ready time:     ${FC_READY_MS:-n/a} ms"
	echo "  Prewarmed claim ready time:  ${WARM_READY_MS} ms"
	echo ""
fi
echo "Useful commands:"
echo "  qarax sandbox list                 # list all sandboxes"
echo "  qarax sandbox get <id>             # inspect a sandbox"
echo "  qarax sandbox create --template T  # create a new sandbox"
echo "  qarax sandbox pool get --template T # inspect the prewarmed pool"
echo "  qarax sandbox exec <id> -- cmd     # run a command inside a sandbox"
echo "  qarax sandbox delete <id>          # delete a sandbox immediately"
echo "  qarax vm-template list             # list all VM templates"
