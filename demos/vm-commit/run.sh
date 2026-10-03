#!/usr/bin/env bash
#
# Demo: VM commit — convert an OCI image-backed VM to a standalone raw disk VM
#
# This script demonstrates the "vm commit" workflow:
#   1. Start the qarax stack (if not already running)
#   2. Use an UP host, or register and initialise qarax-node if there is none
#   3. Create an OverlayBD storage pool (backed by the local registry)
#   4. Create a Local storage pool for the committed raw disk
#   5. Attach both pools to the host (also when reusing pools from a previous run)
#   6. Push a small OCI image (busybox) to the local registry
#   7. Create a VM backed by that OCI image (async job, polls to completion)
#   8. Run `vm commit` to copy the OverlayBD block device to a raw disk
#   9. Verify image_ref is cleared and the committed disk object exists
#
# The demo environment is left running after completion so you can explore it.
# Run with --cleanup to tear down resources created by a previous run.
#
# Prerequisites:
#   - Docker with KVM support (/dev/kvm available)
#   - qarax CLI (built automatically with cargo if missing)
#   - jq, curl, docker
#
# Usage:
#   ./demos/vm-commit/run.sh
#   ./demos/vm-commit/run.sh --cleanup
#   QARAX_SERVER=http://localhost:8000 ./demos/vm-commit/run.sh
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
# Registry URL as seen from outside Docker (host port mapping: 5001 → registry:5000)
REGISTRY_PUSH_URL="${REGISTRY_PUSH_URL:-localhost:5001}"
# Registry URL as seen from inside the Docker network
REGISTRY_INTERNAL_URL="${REGISTRY_INTERNAL_URL:-registry:5000}"
# qarax-node gRPC address as seen from the control plane (inside Docker network)
NODE_ADDRESS="${QARAX_NODE_ADDRESS:-qarax-node}"
NODE_PORT="${QARAX_NODE_PORT:-50051}"

OBD_POOL_NAME="demo-commit-obd"
LOCAL_POOL_NAME="demo-commit-local"
VM_NAME="demo-commit-vm"
IMAGE_TAG="${REGISTRY_PUSH_URL}/demo/busybox:latest"
IMAGE_REF="${REGISTRY_INTERNAL_URL}/demo/busybox:latest"
# Size of committed disk: 1 GiB
COMMIT_SIZE=1073741824
VCPUS=1
MEMORY_BYTES=268435456

banner() {
	echo -e "\n${BOLD}${CYAN}══════════════════════════════════════════════════════════════${NC}"
	echo -e "${BOLD}${CYAN}  $1${NC}"
	echo -e "${BOLD}${CYAN}══════════════════════════════════════════════════════════════${NC}\n"
}
step() { echo -e "${GREEN}▸${NC} ${BOLD}$1${NC}"; }
info() { echo -e "  ${DIM}$1${NC}"; }
run() {
	echo -e "  ${DIM}\$ $*${NC}" >&2
	"$@"
}

# Look up a pool by name; print its ID. Prints nothing if not found.
find_pool() {
	api_curl -sf "${SERVER}/storage-pools?name=$1" | jq -r ".[] | select(.name == \"$1\") | .id"
}

case "${1:-}" in
"" | --cleanup) ;;
--help | -h)
	echo "Usage: $0 [--cleanup]"
	echo "  --cleanup   Delete the VM, storage objects and pools created by a previous run"
	echo "Environment: QARAX_SERVER, QARAX_TOKEN, REGISTRY_PUSH_URL, REGISTRY_INTERNAL_URL,"
	echo "             QARAX_NODE_ADDRESS, QARAX_NODE_PORT"
	exit 0
	;;
*) die "Unknown option: $1 (see --help)" ;;
esac

QARAX_BIN="$(find_qarax_bin)"
if [[ -z "$QARAX_BIN" ]]; then
	echo -e "${YELLOW}qarax CLI not found — building...${NC}"
	cargo build -p cli
	QARAX_BIN="$(find_qarax_bin)"
fi
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"
QARAX="$QARAX_BIN --server $SERVER"

if [[ "${1:-}" == "--cleanup" ]]; then
	banner "Cleaning up demo-commit resources"
	if ! curl -sf --max-time 3 "${SERVER}/" >/dev/null 2>&1; then
		echo "  Stack not running — nothing to clean up."
		exit 0
	fi
	require_auth "$SERVER"

	step "Deleting VM '${VM_NAME}'"
	$QARAX vm delete "$VM_NAME" 2>/dev/null && echo "  Deleted." || echo "  Not found, skipping."

	step "Deleting storage objects in demo pools"
	for pool_name in "$OBD_POOL_NAME" "$LOCAL_POOL_NAME"; do
		pool_id=$(find_pool "$pool_name" 2>/dev/null || true)
		[[ -z "$pool_id" ]] && continue
		for obj_id in $(
			api_curl -sf "${SERVER}/storage-objects?pool_id=${pool_id}" | jq -r '.[].id' 2>/dev/null || true
		); do
			info "Deleting storage object $obj_id"
			$QARAX storage-object delete "$obj_id" 2>/dev/null || true
		done
	done

	step "Deleting storage pools"
	$QARAX storage-pool delete "$OBD_POOL_NAME" 2>/dev/null && echo "  Deleted ${OBD_POOL_NAME}." || echo "  ${OBD_POOL_NAME} not found, skipping."
	$QARAX storage-pool delete "$LOCAL_POOL_NAME" 2>/dev/null && echo "  Deleted ${LOCAL_POOL_NAME}." || echo "  ${LOCAL_POOL_NAME} not found, skipping."

	echo ""
	echo -e "${GREEN}Cleanup complete.${NC}"
	exit 0
fi

ensure_stack "$SERVER"

banner "VM Commit Demo"
echo -e "  Server:           ${SERVER}"
echo -e "  VM name:          ${VM_NAME}"
echo -e "  Image ref:        ${IMAGE_REF}"
echo -e "  OverlayBD pool:   ${OBD_POOL_NAME}"
echo -e "  Local pool:       ${LOCAL_POOL_NAME}"
echo ""

step "Ensure qarax-node host is registered and up"

HOSTS_JSON=$($QARAX host list -o json)
HOST_ID=$(jq -r '[.[] | select(.status == "up")] | .[0].id // empty' <<<"$HOSTS_JSON")

if [[ -z "$HOST_ID" ]]; then
	# Reuse a registered-but-down host at the node address (e.g. after a node
	# restart) rather than tripping over the unique address constraint.
	HOST_ID=$(jq -r --arg a "$NODE_ADDRESS" '[.[] | select(.address == $a)] | .[0].id // empty' <<<"$HOSTS_JSON")
	if [[ -z "$HOST_ID" ]]; then
		info "No UP host found — registering qarax-node at ${NODE_ADDRESS}:${NODE_PORT}..."
		HOST_ID=$(
			run $QARAX host add \
				--name demo-node \
				--address "$NODE_ADDRESS" \
				--port "$NODE_PORT" \
				--user root \
				-o json | jq -r '.host_id'
		)
		info "Registered host: ${HOST_ID}"
	fi
	info "Initialising host ${HOST_ID} (gRPC handshake)..."
	run $QARAX host init "$HOST_ID" >/dev/null
	info "Host initialised."
else
	HOST_NAME=$(jq -r --arg id "$HOST_ID" '.[] | select(.id == $id) | .name' <<<"$HOSTS_JSON")
	info "Found existing UP host: ${HOST_NAME}"
fi

run $QARAX host list
echo ""

step "Push busybox:latest to local registry (${REGISTRY_PUSH_URL})"
info "Pulling busybox:latest from Docker Hub..."
docker pull busybox:latest --quiet
docker tag busybox:latest "${IMAGE_TAG}"
info "Pushing ${IMAGE_TAG}..."
docker push "${IMAGE_TAG}" --quiet
info "Image available at '${IMAGE_REF}' inside the Docker network"
echo ""

step "Create OverlayBD storage pool '${OBD_POOL_NAME}'"
OBD_POOL_ID=$(find_pool "$OBD_POOL_NAME")
if [[ -n "$OBD_POOL_ID" ]]; then
	info "Already exists: ${OBD_POOL_ID}"
	run $QARAX storage-pool attach-host "$OBD_POOL_NAME" "$HOST_ID"
else
	OBD_POOL_ID=$(
		run $QARAX storage-pool create \
			--name "$OBD_POOL_NAME" \
			--pool-type overlaybd \
			--config "{\"url\":\"http://${REGISTRY_INTERNAL_URL}\"}" \
			-o json | jq -r '.pool_id'
	)
	info "Created: ${OBD_POOL_ID}"
	run $QARAX storage-pool attach-host "$OBD_POOL_NAME" "$HOST_ID"
fi
echo ""

step "Create Local storage pool '${LOCAL_POOL_NAME}'"
LOCAL_POOL_ID=$(find_pool "$LOCAL_POOL_NAME")
if [[ -n "$LOCAL_POOL_ID" ]]; then
	info "Already exists: ${LOCAL_POOL_ID}"
	run $QARAX storage-pool attach-host "$LOCAL_POOL_NAME" "$HOST_ID"
else
	LOCAL_POOL_ID=$(
		run $QARAX storage-pool create \
			--name "$LOCAL_POOL_NAME" \
			--pool-type local \
			--config "{\"path\":\"/var/lib/qarax/${LOCAL_POOL_NAME}\"}" \
			--host "$HOST_ID" \
			-o json | jq -r '.pool_id'
	)
	info "Created: ${LOCAL_POOL_ID}"
fi
echo ""

banner "Creating OCI-backed VM"

step "Create VM '${VM_NAME}' with --image-ref"
VM_ID=$(api_curl -sf "${SERVER}/vms?name=${VM_NAME}" | jq -r ".[] | select(.name == \"${VM_NAME}\") | .id")
if [[ -n "$VM_ID" ]]; then
	info "Already exists: ${VM_ID}"
else
	info "This triggers an async provisioning job; the CLI will poll until complete."
	echo ""

	CREATE_JSON=$(
		run $QARAX vm create \
			--name "$VM_NAME" \
			--vcpus "$VCPUS" \
			--memory "$MEMORY_BYTES" \
			--image-ref "$IMAGE_REF" \
			-o json
	)
	VM_ID=$(echo "$CREATE_JSON" | jq -r '.vm_id')
	JOB_ID=$(echo "$CREATE_JSON" | jq -r '.job_id')
	info "VM ID:  ${VM_ID}"
	info "Job ID: ${JOB_ID}"
	echo ""

	step "Polling VM creation job..."
	elapsed=0
	timeout=180
	job_json=""
	status="unknown"
	while [[ $elapsed -lt $timeout ]]; do
		job_json=$(api_curl -sf "${SERVER}/jobs/${JOB_ID}" 2>/dev/null || true)
		status=$(jq -r '.status // "unknown"' <<<"${job_json:-null}" 2>/dev/null || echo "unknown")
		case "$status" in
		completed)
			info "Job completed."
			break
			;;
		failed)
			die "VM creation job failed: $(jq -r '.error // "unknown"' <<<"$job_json")"
			;;
		esac
		sleep 3
		elapsed=$((elapsed + 3))
	done
	[[ "$status" == "completed" ]] ||
		die "Timed out after ${timeout}s waiting for VM creation job ${JOB_ID} (last status: ${status}, progress: $(jq -r '.progress // "?"' <<<"${job_json:-null}" 2>/dev/null))"
fi
echo ""

step "VM state:"
run $QARAX vm get "$VM_NAME"
echo ""

IMAGE_REF_BEFORE=$(api_curl -sf "${SERVER}/vms/${VM_ID}" | jq -r '.image_ref // empty')

if [[ -n "$IMAGE_REF_BEFORE" ]]; then
	info "image_ref = '${IMAGE_REF_BEFORE}'"
	echo ""

	banner "Committing VM to raw disk"

	step "Run 'vm commit' — copy OverlayBD block device to raw disk"
	info "Pool:      ${LOCAL_POOL_NAME}"
	info "Disk size: ${COMMIT_SIZE} bytes (1 GiB)"
	info "The CLI polls the commit job to completion automatically."
	echo ""

	run $QARAX vm commit "$VM_NAME" \
		--storage-pool "$LOCAL_POOL_NAME" \
		--size "$COMMIT_SIZE"
	echo ""
else
	info "image_ref already cleared — commit was run in a previous demo run, skipping."
	echo ""
fi

banner "Verifying commit result"

step "VM state after commit (image_ref should be null)"
run $QARAX vm get "$VM_NAME"
echo ""

IMAGE_REF_AFTER=$(api_curl -sf "${SERVER}/vms/${VM_ID}" | jq -r '.image_ref // empty')
if [[ -n "$IMAGE_REF_AFTER" ]]; then
	die "image_ref was not cleared after commit (got: ${IMAGE_REF_AFTER})"
fi
echo -e "  ${GREEN}PASS${NC} image_ref is null — VM is now standalone (no longer OCI-backed)"
echo ""

step "Committed disk storage object"
run $QARAX storage-object list
echo ""

COMMITTED_OBJ=$(api_curl -sf "${SERVER}/storage-objects?name=committed-${VM_ID}")
COUNT=$(echo "$COMMITTED_OBJ" | jq 'length')
if [[ "$COUNT" -ne 1 ]]; then
	die "Expected 1 committed disk object, got ${COUNT}"
fi
OBJ_TYPE=$(echo "$COMMITTED_OBJ" | jq -r '.[0].object_type')
OBJ_POOL=$(echo "$COMMITTED_OBJ" | jq -r '.[0].storage_pool_id')
[[ "$OBJ_TYPE" == "disk" ]] || die "Expected object_type 'disk', got '${OBJ_TYPE}'"
[[ "$OBJ_POOL" == "$LOCAL_POOL_ID" ]] || die "Expected disk on pool ${LOCAL_POOL_ID}, got ${OBJ_POOL}"

echo -e "  ${GREEN}PASS${NC} committed disk exists  (type=${OBJ_TYPE}, pool=${LOCAL_POOL_NAME})"
echo ""

banner "Demo Complete"
echo -e "  ${GREEN}VM commit workflow verified end-to-end.${NC}"
echo ""
echo -e "  VM '${VM_NAME}' was converted from an OCI OverlayBD image to a"
echo -e "  standalone raw disk stored in pool '${LOCAL_POOL_NAME}'."
echo ""
echo -e "  The demo environment is still running. Explore it (export QARAX_TOKEN first):"
echo -e "    ${DIM}${QARAX} vm get ${VM_NAME}${NC}"
echo -e "    ${DIM}${QARAX} storage-object list --pool ${LOCAL_POOL_NAME}${NC}"
echo -e "    ${DIM}${QARAX} storage-pool list${NC}"
echo ""
echo -e "  To tear down:"
echo -e "    ${DIM}$(realpath "$0") --cleanup${NC}"
echo ""
