#!/usr/bin/env bash
#
# Demo: qarax sandbox feature
#
# Sandboxes are ephemeral VMs spun up from a VM template and automatically
# reaped after an idle timeout.  This demo shows the full lifecycle:
#
#   1. Create Firecracker + Cloud Hypervisor templates backed by one boot source
#   2. Create one sandbox from each template and measure time-to-ready
#   3. Configure a prewarmed sandbox pool for the Firecracker template
#   4. Inspect the Firecracker sandbox (default managed backend)
#   5. Execute a command inside the Firecracker sandbox
#   6. Claim a second Firecracker sandbox from the prewarmed pool
#   7. Delete one sandbox manually
#   8. Watch the remaining sandbox auto-expire after its short idle timeout
#
# With --template, the demo-managed templates and the Firecracker vs Cloud
# Hypervisor benchmark are skipped and the caller's template is used instead.
#
# Each sandbox creates its own underlying VM automatically; no manual VM
# lifecycle management is required.
#
# Prerequisites:
#   - qarax stack (started automatically via hack/run-local.sh if not running)
#   - python3 (JSON parsing); the qarax CLI is built with cargo unless SKIP_BUILD=1
#   - QARAX_TOKEN for the API (defaults to the local stack's e2e-test-token)
#
# Usage:
#   ./demos/sandbox/run.sh
#   ./demos/sandbox/run.sh --server http://localhost:8000
#   ./demos/sandbox/run.sh --template my-template     # reuse an existing template
#   ./demos/sandbox/run.sh --idle-timeout 60          # custom idle timeout in seconds
#   ./demos/sandbox/run.sh --cleanup                  # remove leftover demo resources
#   SANDBOX_INITRAMFS_PATH=/path/to/initramfs ./demos/sandbox/run.sh
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
IDLE_TIMEOUT="${SANDBOX_IDLE_TIMEOUT:-90}"
POOL_MIN_READY="${SANDBOX_POOL_MIN_READY:-1}"
READY_TIMEOUT="${SANDBOX_READY_TIMEOUT:-300}"
HOST_NAME="${QARAX_HOST:-local-node}"
POOL_NAME="sandbox-demo-pool"
POOL_PATH="/var/lib/qarax/images"
KERNEL_PATH="/var/lib/qarax/images/vmlinux"
INITRAMFS_PATH="${SANDBOX_INITRAMFS_PATH:-/var/lib/qarax/images/test-initramfs.gz}"
SANDBOX_NAME_PREFIX="sandbox-demo"
# Demo-managed names get a per-run suffix; the prefixes let --cleanup and the
# pre-run sweep find leftovers from interrupted runs. Note that the Cloud
# Hypervisor template prefix also matches MANAGED_TEMPLATE_PREFIX.
MANAGED_TEMPLATE_PREFIX="sandbox-demo-template"
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
POOL_CONFIGURED=0
ORIG_POOL_MIN_READY=""
MANAGE_TEMPLATE_ASSETS=1
CLEANUP_ONLY=0
STEP_NO=0
COLD_READY_MS=""
CH_READY_MS=""
WARM_READY_MS=""
RUN_SUFFIX="$(date +%s)-$$"

TEMPLATE_NAME="${MANAGED_TEMPLATE_PREFIX}-${RUN_SUFFIX}"
CLOUDHV_TEMPLATE_NAME="${MANAGED_TEMPLATE_PREFIX}-cloudhv-${RUN_SUFFIX}"
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
		echo "  --template NAME       Reuse an existing VM template (skips the managed templates and benchmark)"
		echo "  --idle-timeout SECS   Idle timeout for sandboxes in seconds (default: 90)"
		echo "  --initramfs PATH      Initramfs path on the host running qarax-node"
		echo "  --cleanup             Remove demo-managed sandboxes, VMs, and template assets, then exit"
		echo ""
		echo "Environment: QARAX_TOKEN, QARAX_HOST, SANDBOX_IDLE_TIMEOUT, SANDBOX_POOL_MIN_READY,"
		echo "             SANDBOX_READY_TIMEOUT, SANDBOX_INITRAMFS_PATH, SKIP_BUILD=1"
		exit 0
		;;
	*)
		die "Unknown option: $1 (see --help)"
		;;
	esac
done

[[ -n "$INITRAMFS_PATH" ]] || die "sandbox demo requires an initramfs with qarax-init; set SANDBOX_INITRAMFS_PATH or pass --initramfs"

banner() {
	echo -e "\n${BOLD}${CYAN}══════════════════════════════════════════════════════════════${NC}"
	echo -e "${BOLD}${CYAN}  $1${NC}"
	echo -e "${BOLD}${CYAN}══════════════════════════════════════════════════════════════${NC}\n"
}
next_step() {
	STEP_NO=$((STEP_NO + 1))
	banner "Step ${STEP_NO} — $1"
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
QARAX=("$QARAX_BIN" --server "$SERVER")

cleanup() {
	echo
	step "Cleaning up..."
	local id
	for id in "$SANDBOX1_ID" "$SANDBOX2_ID" "$CLOUDHV_SANDBOX_ID"; do
		if [[ -n "$id" ]]; then
			"${QARAX[@]}" sandbox delete "$id" 2>/dev/null || true
		fi
	done
	# Sandbox and pool deletion are synchronous, so the template assets below
	# are no longer referenced by any VM once these return.
	if [[ "$POOL_CONFIGURED" -eq 1 ]]; then
		if [[ -n "$ORIG_POOL_MIN_READY" ]]; then
			# Caller-managed template already had a pool: restore its setting.
			"${QARAX[@]}" sandbox pool set --template "$TEMPLATE_NAME" --min-ready "$ORIG_POOL_MIN_READY" >/dev/null 2>&1 || true
		else
			"${QARAX[@]}" sandbox pool delete --template "$TEMPLATE_NAME" 2>/dev/null || true
		fi
	fi
	if [[ "$TEMPLATE_CREATED" -eq 1 ]]; then
		"${QARAX[@]}" vm-template delete "$TEMPLATE_NAME" 2>/dev/null || true
	fi
	if [[ "$CLOUDHV_TEMPLATE_CREATED" -eq 1 ]]; then
		"${QARAX[@]}" vm-template delete "$CLOUDHV_TEMPLATE_NAME" 2>/dev/null || true
	fi
	if [[ "$BOOT_SOURCE_CREATED" -eq 1 ]]; then
		"${QARAX[@]}" boot-source delete "$BOOT_SOURCE_NAME" 2>/dev/null || true
	fi
	if [[ "$INITRAMFS_CREATED" -eq 1 ]]; then
		"${QARAX[@]}" storage-object delete "$INITRAMFS_NAME" 2>/dev/null || true
	fi
	if [[ "$KERNEL_CREATED" -eq 1 ]]; then
		"${QARAX[@]}" storage-object delete "$KERNEL_NAME" 2>/dev/null || true
	fi
	info "Done."
}

json_field() {
	local field="$1"
	python3 -c 'import json, sys; print(json.load(sys.stdin)[sys.argv[1]])' "$field"
}

# Print FIELD (name or id) of every KIND resource whose name starts with PREFIX.
list_with_prefix() {
	local kind="$1"
	local prefix="$2"
	local field="$3"
	"${QARAX[@]}" "$kind" list -o json 2>/dev/null | python3 -c '
import json, sys
prefix, field = sys.argv[1], sys.argv[2]
for item in json.load(sys.stdin):
    name = item.get("name") or ""
    if name.startswith(prefix):
        print(item[field])
' "$prefix" "$field" 2>/dev/null || true
}

sandbox_status() {
	local sandbox_id="$1"
	"${QARAX[@]}" sandbox list -o json 2>/dev/null | python3 -c '
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
	"${QARAX[@]}" sandbox pool get --template "$template_name" -o json 2>/dev/null | python3 -c '
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
	names=$(list_with_prefix "$kind" "$prefix" name)
	for name in $names; do
		"${QARAX[@]}" "$kind" delete "$name" >/dev/null 2>&1 || true
	done
}

wait_for_no_matching_ids() {
	local kind="$1"
	local prefix="$2"
	local timeout="$3"
	local elapsed=0
	local ids
	while ((elapsed < timeout)); do
		ids=$(list_with_prefix "$kind" "$prefix" id)
		[[ -z "$ids" ]] && return 0
		sleep 2
		elapsed=$((elapsed + 2))
	done
	return 1
}

wait_for_sandbox_status() {
	local sandbox_id="$1"
	local target="$2"
	local timeout="${3:-$READY_TIMEOUT}"
	local elapsed=0
	local status
	while ((elapsed < timeout)); do
		status=$(sandbox_status "$sandbox_id")
		case "$status" in
		"$target")
			echo ""
			return 0
			;;
		error)
			echo ""
			"${QARAX[@]}" sandbox get "$sandbox_id" >&2 || true
			return 1
			;;
		esac
		echo -ne "\r[${status}]   "
		sleep 2
		elapsed=$((elapsed + 2))
	done
	echo ""
	echo -e "${YELLOW}Timed out after ${timeout}s waiting for sandbox ${sandbox_id} to become ${target} (last status: ${status}).${NC}" >&2
	return 1
}

ensure_clean_demo_state() {
	local sandbox_id vm_id template
	for sandbox_id in $(list_with_prefix sandbox "$SANDBOX_NAME_PREFIX-" id); do
		"${QARAX[@]}" sandbox delete "$sandbox_id" >/dev/null 2>&1 || true
	done
	for vm_id in $(list_with_prefix vm "$SANDBOX_NAME_PREFIX-" id); do
		"${QARAX[@]}" vm delete "$vm_id" >/dev/null 2>&1 || true
	done
	if ! wait_for_no_matching_ids sandbox "$SANDBOX_NAME_PREFIX-" 30; then
		info "Some prior demo sandboxes are still terminating; continuing with fresh run suffixes."
	fi
	if ! wait_for_no_matching_ids vm "$SANDBOX_NAME_PREFIX-" 30; then
		info "Some prior demo VMs are still terminating; continuing with fresh run suffixes."
	fi
	if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
		# A template with a sandbox pool cannot be deleted until the pool (and
		# its standby VMs) is gone. This prefix also covers the CH templates.
		for template in $(list_with_prefix vm-template "$MANAGED_TEMPLATE_PREFIX-" name); do
			"${QARAX[@]}" sandbox pool delete --template "$template" >/dev/null 2>&1 || true
			"${QARAX[@]}" vm-template delete "$template" >/dev/null 2>&1 || true
		done
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
	if ! output=$("${QARAX[@]}" sandbox create \
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
if [[ "$CLEANUP_ONLY" -eq 1 ]]; then
	require_server "$SERVER"
else
	ensure_stack "$SERVER"
fi
info "API reachable at $SERVER"

if [[ "$CLEANUP_ONLY" -eq 1 ]]; then
	banner "Cleanup"
	cleanup_demo_resources
	exit 0
fi

trap cleanup EXIT

if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	info "Using demo-managed template assets ('$MANAGED_TEMPLATE_PREFIX-*')."
else
	info "Using caller-managed template '$TEMPLATE_NAME'."
fi

next_step "VM Templates"

cleanup_demo_resources
echo ""

if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	step "Creating storage pool and boot source for the sandbox templates..."

	if ! "${QARAX[@]}" storage-pool get "$POOL_NAME" >/dev/null 2>&1; then
		run "${QARAX[@]}" storage-pool create \
			--name "$POOL_NAME" \
			--pool-type local \
			--config "{\"path\":\"$POOL_PATH\"}" \
			--host "$HOST_NAME"
		echo ""
	else
		info "Storage pool '$POOL_NAME' already exists."
		run "${QARAX[@]}" storage-pool attach-host "$POOL_NAME" --all
	fi

	# Kernel, initramfs, and boot source names carry this run's suffix, so they
	# never exist yet (leftovers were swept by cleanup_demo_resources above).
	run "${QARAX[@]}" transfer create \
		--pool "$POOL_NAME" \
		--name "$KERNEL_NAME" \
		--source "$KERNEL_PATH" \
		--object-type kernel \
		--wait
	KERNEL_CREATED=1
	echo ""

	run "${QARAX[@]}" transfer create \
		--pool "$POOL_NAME" \
		--name "$INITRAMFS_NAME" \
		--source "$INITRAMFS_PATH" \
		--object-type initrd \
		--wait
	INITRAMFS_CREATED=1
	echo ""

	run "${QARAX[@]}" boot-source create \
		--name "$BOOT_SOURCE_NAME" \
		--kernel "$KERNEL_NAME" \
		--initrd "$INITRAMFS_NAME" \
		--params "console=ttyS0"
	BOOT_SOURCE_CREATED=1
	echo ""

	step "Creating Firecracker default sandbox template '$TEMPLATE_NAME'..."
	echo ""
	run "${QARAX[@]}" vm-template create \
		--name "$TEMPLATE_NAME" \
		--hypervisor firecracker \
		--boot-source "$BOOT_SOURCE_NAME" \
		--vcpus 1 \
		--memory 268435456 \
		--boot-mode kernel
	TEMPLATE_CREATED=1
	echo ""
	TEMPLATE_ID=$("${QARAX[@]}" vm-template get "$TEMPLATE_NAME" -o json | json_field "id")
	info "Firecracker template '$TEMPLATE_NAME' created (id=${TEMPLATE_ID})"

	step "Creating Cloud Hypervisor comparison template '$CLOUDHV_TEMPLATE_NAME'..."
	echo ""
	run "${QARAX[@]}" vm-template create \
		--name "$CLOUDHV_TEMPLATE_NAME" \
		--hypervisor cloud_hv \
		--boot-source "$BOOT_SOURCE_NAME" \
		--vcpus 1 \
		--memory 268435456 \
		--boot-mode kernel
	CLOUDHV_TEMPLATE_CREATED=1
	echo ""
	CLOUDHV_TEMPLATE_ID=$("${QARAX[@]}" vm-template get "$CLOUDHV_TEMPLATE_NAME" -o json | json_field "id")
	info "Cloud Hypervisor comparison template '$CLOUDHV_TEMPLATE_NAME' created (id=${CLOUDHV_TEMPLATE_ID})"
else
	TEMPLATE_ID=$("${QARAX[@]}" vm-template get "$TEMPLATE_NAME" -o json 2>/dev/null | json_field "id") ||
		die "Template '$TEMPLATE_NAME' not found"
	info "Using caller-managed template '$TEMPLATE_NAME' (id=${TEMPLATE_ID})."
fi

# Configure a prewarmed pool for TEMPLATE_NAME and wait until it has
# POOL_MIN_READY standby sandboxes. A pre-existing pool on a caller-managed
# template is restored to its original min_ready by the cleanup trap.
configure_sandbox_pool() {
	if ORIG_POOL_MIN_READY=$("${QARAX[@]}" sandbox pool get --template "$TEMPLATE_NAME" -o json 2>/dev/null | json_field "min_ready"); then
		info "Template already has a sandbox pool (min_ready=${ORIG_POOL_MIN_READY}); it will be restored on exit."
	else
		ORIG_POOL_MIN_READY=""
	fi
	POOL_CONFIGURED=1
	run "${QARAX[@]}" sandbox pool set --template "$TEMPLATE_NAME" --min-ready "$POOL_MIN_READY"
	wait_for_sandbox_pool_ready "$TEMPLATE_NAME" "$POOL_MIN_READY" 120 ||
		die "Sandbox pool for '$TEMPLATE_NAME' did not become ready"
	echo ""
	step "Current sandbox pool status:"
	run "${QARAX[@]}" sandbox pool get --template "$TEMPLATE_NAME"
	echo ""
}

if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	next_step "Provisioning Benchmark"
	info "Benchmarking time-to-ready with identical guest artifacts."
	echo ""

	step "Creating Firecracker sandbox #1 (default backend)..."
	echo ""
	start_ms=$(now_ms)
	SANDBOX1_ID=$(create_sandbox "$TEMPLATE_NAME" "$SANDBOX1_NAME")
	wait_for_sandbox_status "$SANDBOX1_ID" ready || die "Sandbox $SANDBOX1_ID failed to become ready"
	COLD_READY_MS=$(($(now_ms) - start_ms))
	echo ""
	info "Firecracker sandbox ready in ${COLD_READY_MS} ms"
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

	if ((COLD_READY_MS < CH_READY_MS)); then
		info "In this run, Firecracker reached READY $((CH_READY_MS - COLD_READY_MS)) ms faster than Cloud Hypervisor."
	elif ((CH_READY_MS < COLD_READY_MS)); then
		info "In this run, Cloud Hypervisor reached READY $((COLD_READY_MS - CH_READY_MS)) ms faster than Firecracker."
	else
		info "In this run, Firecracker and Cloud Hypervisor reached READY in the same time."
	fi

	next_step "Configure Prewarmed Sandbox Pool"
	step "Keeping ${POOL_MIN_READY} Firecracker sandbox(es) ready for instant claims..."
	echo ""
	configure_sandbox_pool

	next_step "Inspect Firecracker Sandbox"
else
	next_step "Create Sandbox"
	step "Creating sandbox from template '$TEMPLATE_NAME' (idle timeout: ${IDLE_TIMEOUT}s)..."
	echo ""
	start_ms=$(now_ms)
	SANDBOX1_ID=$(create_sandbox "$TEMPLATE_NAME" "$SANDBOX1_NAME")
	wait_for_sandbox_status "$SANDBOX1_ID" ready || die "Sandbox $SANDBOX1_ID failed to become ready"
	COLD_READY_MS=$(($(now_ms) - start_ms))
	echo ""
	info "Sandbox ready in ${COLD_READY_MS} ms"
	echo ""

	next_step "Configure Prewarmed Sandbox Pool"
	step "Keeping ${POOL_MIN_READY} sandbox(es) ready for instant claims..."
	echo ""
	configure_sandbox_pool

	next_step "Inspect Sandbox"
fi

info "Sandbox 1 ID: $SANDBOX1_ID"
echo ""

step "Sandbox 1 details:"
run "${QARAX[@]}" sandbox get "$SANDBOX1_ID"
echo ""

next_step "Execute Inside Sandbox"
step "Running a command inside sandbox #1..."
echo ""
run "${QARAX[@]}" sandbox exec "$SANDBOX1_ID" -- /bin/sh -c 'printf sandbox-demo && uname -s'
echo ""

if [[ -n "$CLOUDHV_SANDBOX_ID" ]]; then
	step "Deleting the Cloud Hypervisor comparison sandbox..."
	echo ""
	run "${QARAX[@]}" sandbox delete "$CLOUDHV_SANDBOX_ID"
	CLOUDHV_SANDBOX_ID=""
	echo ""
fi

next_step "Claim a Prewarmed Sandbox"
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
info "Sandbox 2 ID: $SANDBOX2_ID"
echo ""

step "Sandbox pool after the claim:"
run "${QARAX[@]}" sandbox pool get --template "$TEMPLATE_NAME"
echo ""

step "All sandboxes:"
run "${QARAX[@]}" sandbox list
echo ""

next_step "Manual Delete"

step "Deleting sandbox #1 (${SANDBOX1_ID::8}...) manually..."
echo ""
run "${QARAX[@]}" sandbox delete "$SANDBOX1_ID"
SANDBOX1_ID=""
echo ""

step "Remaining sandboxes:"
run "${QARAX[@]}" sandbox list
echo ""

next_step "Auto-Reap via Idle Timeout"

max_wait=$((IDLE_TIMEOUT + 60))
info "Sandbox #2 has an idle timeout of ${IDLE_TIMEOUT}s."
info "The sandbox reaper runs periodically (every 10s by default) and destroys it once the timeout expires."
info "Waiting for sandbox #2 to disappear (up to ${max_wait}s)..."
echo ""

elapsed=0
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
if [[ "$MANAGE_TEMPLATE_ASSETS" -eq 1 ]]; then
	echo "  ✓ Create sandbox templates from a shared boot source"
	echo "  ✓ Default managed sandboxes to Firecracker"
	echo "  ✓ Benchmark Firecracker vs Cloud Hypervisor time-to-ready"
else
	echo "  ✓ Create a sandbox from an existing template"
fi
echo "  ✓ Prewarm a sandbox pool"
echo "  ✓ Inspect a running sandbox"
echo "  ✓ Execute a command inside the sandbox over the guest agent"
echo "  ✓ Claim a second sandbox from the prewarmed pool"
echo "  ✓ Delete a sandbox manually"
echo "  ✓ Watch idle-timeout auto-reap kick in"
echo ""
echo "Measured performance in this run:"
if [[ -n "$CH_READY_MS" ]]; then
	echo "  Firecracker ready time:      ${COLD_READY_MS} ms"
	echo "  Cloud Hypervisor ready time: ${CH_READY_MS} ms"
else
	echo "  Cold sandbox ready time:     ${COLD_READY_MS} ms"
fi
echo "  Prewarmed claim ready time:  ${WARM_READY_MS} ms"
echo ""
echo "Useful commands (export QARAX_TOKEN first):"
echo "  qarax sandbox list                  # list all sandboxes"
echo "  qarax sandbox get <id>              # inspect a sandbox"
echo "  qarax sandbox create --template T   # create a new sandbox"
echo "  qarax sandbox pool get --template T # inspect the prewarmed pool"
echo "  qarax sandbox exec <id> -- cmd      # run a command inside a sandbox"
echo "  qarax sandbox keepalive <id>        # reset the idle timer"
echo "  qarax sandbox delete <id>           # delete a sandbox immediately"
echo "  qarax vm-template list              # list all VM templates"
