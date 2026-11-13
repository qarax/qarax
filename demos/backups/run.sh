#!/usr/bin/env bash
#
# Demo: top-level backups
#
# Shows the `qarax backup` surface end-to-end:
#   1. VM backup create/list/get/restore/delete, proving the restore brought
#      back the guest's state: a file written inside the guest before the
#      backup reads the same after a stop and restore.
#   2. Control-plane database backup create/list/get/restore, proving the
#      restore rewound the database.
#
# Prerequisites:
#   - qarax stack running (make run-local; started automatically if not up)
#   - docker (to clear dump files inside the qarax container), jq
#
# Usage:
#   ./demos/backups/run.sh [--server URL] [--pool-name NAME] [--pool-path PATH]
#   ./demos/backups/run.sh --cleanup   # delete everything the demo made, pool included
#   KEEP=1 ./demos/backups/run.sh      # leave resources in place on failure
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-backups-"
VM_NAME="${PREFIX}vm"
VM_BACKUP_NAME="${PREFIX}vm-backup"
DB_BACKUP_NAME="${PREFIX}db-backup"
PRE_TYPE_NAME="${PREFIX}before"
POST_TYPE_NAME="${PREFIX}after"
POOL_NAME="${POOL_NAME:-${PREFIX}local-pool}"
POOL_PATH="${POOL_PATH:-/tmp/qarax-demo-backups}"
# The guest's root filesystem is its initramfs, in RAM: once the VM stops,
# only a restore of the backup's memory snapshot can bring this file back.
MARKER_FILE="/root/backup-marker"
MEMORY_BYTES=$((256 * 1024 * 1024))
CLEANUP_ONLY=0

while [[ $# -gt 0 ]]; do
	case "$1" in
	--server)
		SERVER="$2"
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
	--cleanup)
		CLEANUP_ONLY=1
		shift
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo ""
		echo "Options:"
		echo "  --server URL       qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --pool-name NAME   Reusable local storage pool name (default: demo-backups-local-pool)"
		echo "  --pool-path PATH   Writable control-plane-local path for DB dumps (default: /tmp/qarax-demo-backups)"
		echo "  --cleanup          Delete the demo's backups, VM, instance types and pool, then exit"
		echo ""
		echo "KEEP=1 leaves the demo's resources in place at exit."
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

backup_status() {
	qarax backup get "$1" -o json | jq -r '.status'
}

# The control plane writes database dumps into the pool path inside the qarax
# container: create it there, and remove dumps whose records a database
# restore dropped.
clear_dumps() {
	node_exec qarax sh -c "mkdir -p '${POOL_PATH}' && rm -f '${POOL_PATH}'/*.dump"
}

# Backups first: deleting a VM drops its backup records but leaves the
# snapshot data on the node.
remove_leftovers() {
	sweep backup "$PREFIX"
	sweep vm "$PREFIX"
	sweep instance-type "$PREFIX"
	clear_dumps
}

ensure_backup_pool() {
	local pool_json host
	pool_json=$(qarax storage-pool list -o json |
		jq -c --arg name "$POOL_NAME" '[.[] | select(.name == $name)][0] // empty')

	if [[ -z "$pool_json" ]]; then
		host=$(first_up_host)
		run qarax storage-pool create \
			--name "$POOL_NAME" \
			--pool-type local \
			--path "$POOL_PATH" \
			--host "$host"
	else
		[[ "$(jq -r '.pool_type' <<<"$pool_json")" == "local" ]] ||
			die "Storage pool '${POOL_NAME}' exists but is not a local pool"
		[[ "$(jq -r '.config.path // empty' <<<"$pool_json")" == "$POOL_PATH" ]] ||
			die "Storage pool '${POOL_NAME}' uses path '$(jq -r '.config.path' <<<"$pool_json")', expected '${POOL_PATH}'"
		info "Reusing storage pool '${POOL_NAME}'."
	fi
	# The VM may be scheduled on any UP host, and its snapshot is written there.
	run qarax storage-pool attach-host "$POOL_NAME" --all
}

command -v jq >/dev/null 2>&1 || die "jq is required"
command -v docker >/dev/null 2>&1 || die "docker is required"

ensure_stack
require_cli

if ((CLEANUP_ONLY)); then
	remove_leftovers
	if qarax storage-pool list -o json | jq -e --arg name "$POOL_NAME" 'any(.[]; .name == $name)' >/dev/null; then
		run qarax storage-pool delete "$POOL_NAME"
	fi
	ok "Removed the backups demo's resources"
	exit 0
fi

banner "Top-level Backups Demo"

step "Removing leftovers from earlier runs..."
remove_leftovers

cleanup() {
	echo
	if [[ "$KEEP" == 1 ]]; then
		info "KEEP=1: leaving the demo's resources in place (remove them with: $0 --cleanup)"
		return
	fi
	step "Cleaning up..."
	remove_leftovers
	info "Left the reusable storage pool '${POOL_NAME}' in place."
}
trap cleanup EXIT

step "Preparing reusable local backup storage at '${POOL_PATH}'..."
ensure_backup_pool

banner "Part 1: VM backup create/list/get/restore/delete"

step "Creating and starting a VM with the guest agent..."
run qarax vm create --name "$VM_NAME" --vcpus 1 --memory 256MiB --guest-agent
run qarax vm start "$VM_NAME"
wait_guest_agent "$VM_NAME"

step "Writing a marker file inside the guest..."
run qarax vm exec "$VM_NAME" -- /bin/sh -c "echo before-backup > ${MARKER_FILE}"

step "Creating a VM backup (a snapshot of the running VM, memory included)..."
run qarax backup create vm --vm "$VM_NAME" --name "$VM_BACKUP_NAME" --pool "$POOL_NAME"
[[ "$(backup_status "$VM_BACKUP_NAME")" == "ready" ]] || die "VM backup '${VM_BACKUP_NAME}' is not ready"

step "Listing and inspecting the VM backup..."
run qarax backup list --type vm --name "$VM_BACKUP_NAME"
run qarax backup get "$VM_BACKUP_NAME"

step "Overwriting the marker, then stopping the VM (its RAM, and so the marker, is gone)..."
run qarax vm exec "$VM_NAME" -- /bin/sh -c "echo after-backup > ${MARKER_FILE}"
run qarax vm stop "$VM_NAME"

step "Restoring the VM from the backup..."
run qarax backup restore "$VM_BACKUP_NAME"
wait_guest_agent "$VM_NAME"
MARKER=$(capture qarax vm exec "$VM_NAME" -- /bin/cat "$MARKER_FILE")
echo "    ${MARKER}"
[[ "$MARKER" == "before-backup" ]] ||
	die "The marker reads '${MARKER}' after the restore, expected 'before-backup'"
ok "The guest is back in its state at backup time"

step "Deleting the VM backup (removes its snapshot from the node)..."
run qarax backup delete "$VM_BACKUP_NAME"

step "Deleting the demo VM before the database part..."
run qarax vm delete "$VM_NAME"

banner "Part 2: Database backup create/list/get/restore"

step "Creating control-plane state that should survive the restore..."
run qarax instance-type create \
	--name "$PRE_TYPE_NAME" \
	--vcpus 1 \
	--max-vcpus 1 \
	--memory "$MEMORY_BYTES"

step "Creating a database backup..."
run qarax backup create database --name "$DB_BACKUP_NAME" --pool "$POOL_NAME"
[[ "$(backup_status "$DB_BACKUP_NAME")" == "ready" ]] || die "Database backup '${DB_BACKUP_NAME}' is not ready"

step "Listing and inspecting the database backup..."
run qarax backup list --type database --name "$DB_BACKUP_NAME"
run qarax backup get "$DB_BACKUP_NAME"

step "Creating control-plane state that should be rolled back..."
run qarax instance-type create \
	--name "$POST_TYPE_NAME" \
	--vcpus 1 \
	--max-vcpus 1 \
	--memory $((512 * 1024 * 1024))

step "Restoring the control-plane database backup..."
run qarax backup restore "$DB_BACKUP_NAME"

step "Verifying the restore rewound control-plane state..."
run qarax instance-type get "$PRE_TYPE_NAME"
if qarax instance-type list -o json | jq -e --arg name "$POST_TYPE_NAME" 'any(.[]; .name == $name)' >/dev/null; then
	die "Instance type '${POST_TYPE_NAME}' still exists after the database restore"
fi
[[ "$(qarax backup list --type database --name "$DB_BACKUP_NAME" -o json | jq 'length')" == "0" ]] ||
	die "The database restore did not rewind backup metadata as expected"
ok "Pre-backup data survived; post-backup data was removed."
info "The database backup's own record is gone too: it was written after the dump it describes."

step "Removing the surviving instance type and the leftover dump..."
run qarax instance-type delete "$PRE_TYPE_NAME"
clear_dumps

banner "Demo Complete"
