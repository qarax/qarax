#!/usr/bin/env bash
#
# Demo: BLOCK (iSCSI) storage pool backed by LIO targetcli
#
# Spins up a LIO iSCSI target container in the qarax compose network, registers
# it in qarax as a BLOCK storage pool (which auto-attaches to every UP host, so
# qarax-node logs in to the target), and registers the target's LUN 0 as a disk
# object.
#
# Prerequisites:
#   - local qarax stack in container mode (make run-local); started via
#     hack/run-local.sh if not running
#   - jq, docker with docker compose
#   - host kernel with LIO (target_core_mod, iscsi_target_mod) and iscsi_tcp
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

cd "$REPO_ROOT"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
KEEP_RESOURCES=false
# Interpolated by e2e/docker-compose.yml, which this demo merges with its own.
export CLOUD_HYPERVISOR_VERSION="${CLOUD_HYPERVISOR_VERSION:-$(tr -d '\n' <"${REPO_ROOT}/versions/cloud-hypervisor-version")}"
export FIRECRACKER_VERSION="${FIRECRACKER_VERSION:-$(tr -d '\n' <"${REPO_ROOT}/versions/firecracker-version")}"

# Must match demos/block-storage/compose.yml.
TARGET_IQN="iqn.2024-01.io.qarax:demo"
LUN_SIZE_BYTES=1073741824
PORTAL="iscsi-target:3260"
NODE_SERVICE="qarax-node"

SUFFIX="$$"
POOL_NAME="block-demo-${SUFFIX}"
DISK_NAME="block-demo-lun0-${SUFFIX}"

POOL_ID=""
DISK_ID=""
TARGET_STARTED=false

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

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --server URL       qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)
  --keep-resources   Keep the pool, disk object and iSCSI target running on exit
  --help, -h         Show this help
EOF
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--server)
		SERVER="${2:?--server requires a URL}"
		shift 2
		;;
	--keep-resources)
		KEEP_RESOURCES=true
		shift
		;;
	--help | -h)
		usage
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

if [[ -z "$(find_qarax_bin)" ]]; then
	echo "qarax CLI not found — building..."
	cargo build -p cli
fi

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"

qarax() {
	"$QARAX_BIN" --server "$SERVER" "$@"
}

COMPOSE_ARGS=(-f "${REPO_ROOT}/e2e/docker-compose.yml" -f "${REPO_ROOT}/demos/block-storage/compose.yml")
TEARDOWN_CMD="docker compose -f e2e/docker-compose.yml -f demos/block-storage/compose.yml rm -sf iscsi-target"

docker_compose() {
	docker compose "${COMPOSE_ARGS[@]}" "$@"
}

target_listening() {
	docker_compose exec -T iscsi-target ss -ltn 2>/dev/null | grep -q ':3260'
}

node_has_session() {
	docker_compose exec -T "$NODE_SERVICE" iscsiadm -m session 2>/dev/null | grep -qF "$TARGET_IQN"
}

cleanup() {
	if [[ "$KEEP_RESOURCES" == "true" ]]; then
		echo
		step "Keeping demo resources for inspection"
		info "Pool: ${POOL_NAME} ${POOL_ID:+(${POOL_ID})}"
		info "Disk object: ${DISK_NAME} ${DISK_ID:+(${DISK_ID})}"
		info "Tear down: delete the disk object, detach the pool from every host"
		info "(qarax storage-pool detach-host ${POOL_NAME} <host>), delete the pool,"
		info "then stop the target: ${TEARDOWN_CMD}"
		return
	fi

	echo
	step "Cleaning up..."

	if [[ -n "$DISK_ID" ]]; then
		qarax storage-object delete "$DISK_ID" >/dev/null 2>&1 || true
	fi

	if [[ -n "$POOL_ID" ]]; then
		# Detaching logs each node out of the target. Do it before stopping the
		# target, or the host kernel keeps retrying a dead iSCSI session.
		local host_id
		while read -r host_id; do
			[[ -n "$host_id" ]] || continue
			qarax storage-pool detach-host "$POOL_ID" "$host_id" >/dev/null 2>&1 || true
		done < <(qarax host list -o json 2>/dev/null | jq -r '.[] | select(.status == "up") | .id' 2>/dev/null || true)
		qarax storage-pool delete "$POOL_ID" >/dev/null 2>&1 || true
	fi

	if [[ "$TARGET_STARTED" == "true" ]]; then
		docker_compose rm -sf iscsi-target >/dev/null 2>&1 || true
	fi

	info "Done."
}
trap cleanup EXIT

banner "BLOCK (iSCSI) Storage Pool Demo"

step "Preflight checks"
command -v jq >/dev/null || die "jq is required"
command -v docker >/dev/null || die "docker is required"
docker compose version >/dev/null 2>&1 || die "docker compose is required"

ensure_stack "$SERVER"

docker_compose ps --status running --services 2>/dev/null | grep -qx "$NODE_SERVICE" ||
	die "The ${NODE_SERVICE} container is not running. This demo needs the container-mode stack ('make run-local', not --vm)."

step "Starting the LIO iSCSI target container"
TARGET_STARTED=true
run docker_compose up -d --build iscsi-target

elapsed=0
until target_listening; do
	if [[ $elapsed -ge 60 ]]; then
		docker_compose logs --tail 30 iscsi-target >&2 || true
		die "iSCSI target did not start listening on port 3260 within 60s"
	fi
	sleep 2
	elapsed=$((elapsed + 2))
done
info "iSCSI target ${TARGET_IQN} listening on ${PORTAL}"

# The node entrypoint (e2e/entrypoint-qarax-node.sh) starts iscsid; containers
# created before it did so need recreating.
docker_compose exec -T "$NODE_SERVICE" sh -c 'cat /proc/[0-9]*/comm 2>/dev/null | grep -qx iscsid' ||
	die "iscsid is not running in ${NODE_SERVICE}.\nRecreate the node container: docker compose -f e2e/docker-compose.yml up -d --build --force-recreate ${NODE_SERVICE}"

step "Creating the BLOCK storage pool"
info "\$ qarax storage-pool create --name ${POOL_NAME} --pool-type block --portal ${PORTAL} --iqn ${TARGET_IQN} --capacity ${LUN_SIZE_BYTES}"
POOL_ID="$(qarax -o json storage-pool create \
	--name "$POOL_NAME" \
	--pool-type block \
	--portal "$PORTAL" \
	--iqn "$TARGET_IQN" \
	--capacity "$LUN_SIZE_BYTES" | jq -r '.pool_id')"
[[ -n "$POOL_ID" && "$POOL_ID" != "null" ]] || die "Failed to read the new pool ID"
info "Pool ID: ${POOL_ID}"
run qarax storage-pool get "$POOL_ID"
echo

step "Checking whether ${NODE_SERVICE} logged in to the target"
info "Shared pools auto-attach to every UP host in the background (iscsiadm discovery + login)."
# In the compose stack qarax-node runs in a Docker bridge network, and the
# kernel's iSCSI initiator netlink interface only works in the host network
# namespace, so login fails there (iscsid logs "sendmsg: bug? ctrl_fd").
# Give it a few seconds in case the node does run in the host namespace.
SESSION=false
for _ in 1 2 3 4 5; do
	if node_has_session; then
		SESSION=true
		break
	fi
	sleep 2
done
if [[ "$SESSION" == "true" ]]; then
	run docker_compose exec -T "$NODE_SERVICE" iscsiadm -m session
	docker_compose exec -T "$NODE_SERVICE" iscsiadm -m session -P 3 2>/dev/null |
		grep -E 'Target:|Attached scsi disk' || true
else
	echo -e "  ${YELLOW}No iSCSI session: expected in the container-mode stack.${NC}"
	info "The kernel only accepts iSCSI logins from the host network namespace, and"
	info "${NODE_SERVICE} runs in a Docker bridge network. On a real hypervisor host"
	info "(bootc appliance or bare metal) the pool attaches and each LUN appears under"
	info "/dev/disk/by-path/. The pool and LUN APIs below work either way."
fi
echo

step "Registering LUN 0 as a disk object"
info "\$ qarax storage-pool register-lun --pool ${POOL_ID} --name ${DISK_NAME} --lun 0 --size ${LUN_SIZE_BYTES}"
DISK_ID="$(qarax -o json storage-pool register-lun \
	--pool "$POOL_ID" \
	--name "$DISK_NAME" \
	--lun 0 \
	--size "$LUN_SIZE_BYTES" | jq -r '.storage_object_id')"
[[ -n "$DISK_ID" && "$DISK_ID" != "null" ]] || die "Failed to read the new disk object ID"
run qarax storage-object get "$DISK_ID"

banner "Demo Complete"
info "BLOCK pool ${POOL_NAME} points at ${TARGET_IQN} on ${PORTAL}."
if [[ "$SESSION" == "true" ]]; then
	info "${NODE_SERVICE} logged in to the target when the pool auto-attached."
else
	info "${NODE_SERVICE} could not log in (container network namespace; see above)."
fi
info "LUN 0 (1 GiB) is registered as disk object ${DISK_NAME}."
if [[ "$KEEP_RESOURCES" == "true" ]]; then
	info "Attach it to a VM with: qarax vm attach-disk <vm> --object ${DISK_NAME}"
fi
