#!/usr/bin/env bash
#
# Demo: BLOCK (iSCSI) storage pool backed by LIO targetcli
#
# Starts the local stack's LIO iSCSI target (compose service iscsi-target),
# registers it in qarax as a BLOCK storage pool (shared pools auto-attach to
# every UP host), and registers the target's LUN 0 as a disk object.
#
# With the node as a bootc microVM (make run-local VM=1), the node logs in to
# the target and LUN 0 appears on it. The container stack's node can't log in
# from its network namespace (see README.md), so there the demo exercises the
# API only.
#
# Prerequisites:
#   - local qarax stack (make run-local, or make run-local VM=1); started via
#     hack/run-local.sh if not running
#   - jq, docker with docker compose
#   - host kernel with LIO (target_core_mod, iscsi_target_mod)
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

# Must match the iscsi-target service in e2e/docker-compose.yml.
TARGET_IQN="iqn.2024-01.io.qarax:e2e"
LUN_SIZE_BYTES=1073741824

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

# The local stack's compose project, with the iscsi-target service's profile.
block_compose() {
	compose --profile block "$@"
}

# Whether the demo pool is attached to host $1.
pool_attached_to() {
	qarax storage-pool hosts "$POOL" -o json | jq -e --arg h "$1" 'any(.[]; .host_name == $h)' >/dev/null
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
# compose service: a node container or the bootc microVM.
host=$(first_up_host)
address=$(qarax host get "$host" -o json | jq -r '.address')
service=$(node_service_for_address "$address") ||
	die "host ${host} (${address}) is not a local node. This demo needs the local stack ('make run-local' or 'make run-local VM=1'): the iSCSI target is only reachable on the compose network."

# A target left running by a KEEP=1 run is reused as is.
step "Clearing leftovers from earlier runs"
sweep_qarax
trap cleanup EXIT

step "Starting the LIO iSCSI target container (compose service iscsi-target)"
block_compose up -d --build --wait iscsi-target ||
	die "iscsi-target did not become healthy (listening on 3260); see: docker compose -f e2e/docker-compose.yml --profile block logs iscsi-target"
# A portal is an IP address: udev names a LUN's device after the address the
# node logged in to, and that is how a VM's disk is found.
target_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$(block_compose ps -q iscsi-target)")
[[ -n "$target_ip" ]] || die "the iscsi-target container has no address"
PORTAL="${target_ip}:3260"
ok "iSCSI target ${TARGET_IQN} listening on ${PORTAL}"

step "Creating the BLOCK storage pool"
run qarax storage-pool create --name "$POOL" --pool-type block \
	--portal "$PORTAL" --iqn "$TARGET_IQN" --capacity "$LUN_SIZE_BYTES"
run qarax storage-pool get "$POOL"
info "Shared pools auto-attach to every UP host in the background (iscsiadm discovery + login)."
if [[ "$service" == bootc-vm ]]; then
	wait_until 120 pool_attached_to "$host" ||
		die "${POOL} did not attach to ${host}; see: docker compose -f e2e/docker-compose.yml exec bootc-vm node-ssh journalctl -u qarax-node -u iscsid"
	run qarax storage-pool hosts "$POOL"
	run node_exec bootc-vm iscsiadm -m session
	device="/dev/disk/by-path/ip-${PORTAL}-iscsi-${TARGET_IQN}-lun-0"
	run node_exec bootc-vm ls -l "$device"
	ok "${host} is logged in to the target; LUN 0 is ${device} there"
else
	info "The kernel only accepts iSCSI logins from the host network namespace, and the"
	info "compose qarax-node runs in a Docker bridge network, so no session forms here."
	info "With the node as a bootc microVM (make run-local VM=1) the pool attaches and"
	info "each LUN appears under /dev/disk/by-path/."
fi

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
