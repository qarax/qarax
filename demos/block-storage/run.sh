#!/usr/bin/env bash
#
# Demo: BLOCK (iSCSI) storage pool backed by LIO targetcli
#
# Starts a LIO iSCSI target container in the qarax compose network, registers
# it in qarax as a BLOCK storage pool (shared pools auto-attach to every UP
# host), and registers the target's LUN 0 as a disk object.
#
# On the local container stack this exercises the API only: the node can't log
# in to the target from a container network namespace (see README.md).
#
# Prerequisites:
#   - local qarax stack in container mode (make run-local); started via
#     hack/run-local.sh if not running
#   - jq, docker with docker compose
#   - host kernel with LIO (target_core_mod, iscsi_target_mod)
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

# Must match demos/block-storage/compose.yml.
TARGET_IQN="iqn.2024-01.io.qarax:demo"
LUN_SIZE_BYTES=1073741824
PORTAL="iscsi-target:3260"

PREFIX="demo-block-"
POOL="${PREFIX}pool"
DISK="${PREFIX}lun0"

CLEANUP_ONLY=0

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --cleanup      Delete the pool, disk object and iSCSI target left by an earlier run, then exit
  --server URL   qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)
  --help, -h     Show this help

Environment:
  KEEP=1         Leave the pool, disk object and iSCSI target in place at exit
EOF
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--cleanup)
		CLEANUP_ONLY=1
		shift
		;;
	--server)
		SERVER="${2:?--server requires a URL}"
		shift 2
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

# The local stack's compose project plus this demo's iscsi-target service.
block_compose() {
	compose -f "${REPO_ROOT}/demos/block-storage/compose.yml" "$@"
}

# Detach each demo pool from every UP host, then delete it. Detaching logs the
# node out of the target (a no-op where it never logged in); do it before the
# target stops, or the host kernel keeps retrying a dead iSCSI session.
sweep_pools() {
	local hosts pool host
	hosts=$(qarax host list -o json | jq -r '.[] | select(.status == "up") | .id')
	for pool in $(qarax storage-pool list -o json |
		jq -r --arg p "$PREFIX" '.[] | select(.name | startswith($p)) | .id'); do
		for host in $hosts; do
			qarax storage-pool detach-host "$pool" "$host" >/dev/null
		done
		qarax storage-pool delete "$pool" >/dev/null
	done
}

sweep_qarax() {
	sweep storage-object "$PREFIX"
	sweep_pools
}

sweep_all() {
	sweep_qarax
	block_compose rm -sf iscsi-target >/dev/null
}

cleanup() {
	if [[ "$KEEP" == 1 ]]; then
		info "KEEP=1: leaving pool ${POOL}, disk object ${DISK} and the iscsi-target container. Remove them with: $0 --cleanup"
		return
	fi
	step "Cleaning up"
	sweep_all
}

banner "BLOCK (iSCSI) Storage Pool Demo"

command -v jq >/dev/null || die "jq is required"
ensure_stack
require_cli

if ((CLEANUP_ONLY)); then
	sweep_all
	ok "Removed ${PREFIX}* resources and the iscsi-target container"
	exit 0
fi

# The target is only reachable on the compose network, so the node must be a
# compose service.
host=$(first_up_host)
address=$(qarax host get "$host" -o json | jq -r '.address')
service=$(node_service_for_address "$address") && [[ "$service" != bootc-vm ]] ||
	die "host ${host} (${address}) is not a local node container. This demo needs the container-mode stack ('make run-local', not VM=1): the iSCSI target is only reachable on the compose network."

# A target left running by a KEEP=1 run is reused as is.
step "Clearing leftovers from earlier runs"
sweep_qarax
trap cleanup EXIT

step "Starting the LIO iSCSI target container (compose service iscsi-target)"
block_compose up -d --build --wait iscsi-target ||
	die "iscsi-target did not become healthy (listening on 3260); see: docker compose -f e2e/docker-compose.yml -f demos/block-storage/compose.yml logs iscsi-target"
ok "iSCSI target ${TARGET_IQN} listening on ${PORTAL}"

step "Creating the BLOCK storage pool"
run qarax storage-pool create --name "$POOL" --pool-type block \
	--portal "$PORTAL" --iqn "$TARGET_IQN" --capacity "$LUN_SIZE_BYTES"
run qarax storage-pool get "$POOL"
info "Shared pools auto-attach to every UP host in the background (iscsiadm discovery + login)."
info "The kernel only accepts iSCSI logins from the host network namespace, and the"
info "compose qarax-node runs in a Docker bridge network, so no session forms here."
info "On a real hypervisor host (bootc appliance or bare metal) the pool attaches and"
info "each LUN appears under /dev/disk/by-path/."

step "Registering LUN 0 as a disk object"
run qarax storage-pool register-lun --pool "$POOL" --name "$DISK" --lun 0 --size "$LUN_SIZE_BYTES"
run qarax storage-object get "$DISK"
pool_id=$(qarax storage-pool get "$POOL" -o json | jq -r '.id')
qarax storage-object get "$DISK" -o json |
	jq -e --arg pool "$pool_id" --argjson size "$LUN_SIZE_BYTES" \
		'.storage_pool_id == $pool and .object_type == "disk" and .size_bytes == $size' >/dev/null ||
	die "${DISK} is not a ${LUN_SIZE_BYTES}-byte disk object in ${POOL}"
ok "LUN 0 is registered as disk object ${DISK} in ${POOL}"

banner "Demo Complete"
ok "BLOCK pool ${POOL} points at ${TARGET_IQN} on ${PORTAL}"
ok "LUN 0 (1 GiB) is registered as disk object ${DISK}"
