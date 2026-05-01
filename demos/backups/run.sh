#!/usr/bin/env bash
#
# Demo: top-level backups
#
# Shows the first-class `qarax backup` surface end-to-end:
#   1. VM backup create/list/get/restore
#   2. Control-plane database backup create/list/get/restore
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
POOL_NAME="${POOL_NAME:-demo-backups-local-pool}"
POOL_PATH="${POOL_PATH:-/tmp/qarax-demo-backups}"
HOST_NAME=""

VM_NAME="backups-demo-vm-$$"
VM_BACKUP_NAME="backups-demo-vm-backup-$$"
DB_BACKUP_NAME="backups-demo-db-backup-$$"
PRE_TYPE_NAME="backups-demo-before-$$"
POST_TYPE_NAME="backups-demo-after-$$"
MEMORY_BYTES=$((256 * 1024 * 1024))

VM_CREATED=0
PRE_TYPE_CREATED=0
POST_TYPE_CREATED=0

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
	--pool-name)
		POOL_NAME="$2"
		shift 2
		;;
	--pool-path)
		POOL_PATH="$2"
		shift 2
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo ""
		echo "Options:"
		echo "  --server URL       qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --host NAME        Host to attach the reusable local backup pool to"
		echo "  --pool-name NAME   Reusable local storage pool name (default: demo-backups-local-pool)"
		echo "  --pool-path PATH   Writable control-plane-local path for DB dumps (default: /tmp/qarax-demo-backups)"
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

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
capture() {
	echo -e "  ${DIM}\$ $*${NC}" >&2
	"$@"
}

wait_for_api() {
	local timeout="${1:-60}"
	local elapsed=0
	while [[ "$elapsed" -lt "$timeout" ]]; do
		if curl -sf --max-time 3 "${SERVER}/" >/dev/null 2>&1; then
			return 0
		fi
		sleep 2
		elapsed=$((elapsed + 2))
	done
	die "qarax API did not become ready at ${SERVER}"
}

wait_for_vm_status() {
	local vm="$1"
	local expected="$2"
	local timeout="${3:-60}"
	local elapsed=0
	local status=""

	while [[ "$elapsed" -lt "$timeout" ]]; do
		status=$("${QARAX[@]}" vm get "$vm" -o json | jq -r '.status')
		if [[ "$status" == "$expected" ]]; then
			return 0
		fi
		sleep 2
		elapsed=$((elapsed + 2))
	done

	die "VM '$vm' did not reach status '$expected' (last status: '${status:-unknown}')"
}

cleanup_backup_artifacts() {
	local qarax_container
	qarax_container=$(docker ps \
		--filter label=com.docker.compose.project=e2e \
		--filter label=com.docker.compose.service=qarax \
		--format '{{.ID}}' | head -n 1)
	[[ -n "$qarax_container" ]] || die "Could not find the running qarax control-plane container"
	docker exec "$qarax_container" sh -lc "mkdir -p '${POOL_PATH}' && rm -f '${POOL_PATH}'/*.dump"
}

ensure_backup_pool() {
	local pool_id pool_json pool_type pool_path

	if "${QARAX[@]}" storage-pool get "$POOL_NAME" -o json >/dev/null 2>&1; then
		pool_id=$(curl -sf "${SERVER}/storage-pools" | jq -r ".[] | select(.name == \"${POOL_NAME}\") | .id")
		[[ -n "$pool_id" ]] || die "Storage pool '$POOL_NAME' exists but could not be resolved via the API"
		pool_json=$(curl -sf "${SERVER}/storage-pools/${pool_id}")
		pool_type=$(jq -r '.pool_type' <<<"$pool_json")
		pool_path=$(jq -r '.config.path // empty' <<<"$pool_json")
		[[ "$pool_type" == "local" ]] || die "Storage pool '$POOL_NAME' exists but is not a local pool"
		[[ "$pool_path" == "$POOL_PATH" ]] || die "Storage pool '$POOL_NAME' uses path '$pool_path', expected '$POOL_PATH'"
		info "Reusing storage pool '$POOL_NAME'."
	else
		run "${QARAX[@]}" storage-pool create \
			--name "$POOL_NAME" \
			--pool-type local \
			--path "$POOL_PATH" \
			--host "$HOST_NAME"
	fi

	run "${QARAX[@]}" storage-pool attach-host "$POOL_NAME" "$HOST_NAME"
}

cleanup() {
	echo
	step "Cleaning up..."
	if [[ "$VM_CREATED" -eq 1 ]]; then
		"${QARAX[@]}" vm stop "$VM_NAME" >/dev/null 2>&1 || true
		"${QARAX[@]}" vm delete "$VM_NAME" >/dev/null 2>&1 || true
	fi
	if [[ "$POST_TYPE_CREATED" -eq 1 ]]; then
		"${QARAX[@]}" instance-type delete "$POST_TYPE_NAME" >/dev/null 2>&1 || true
	fi
	if [[ "$PRE_TYPE_CREATED" -eq 1 ]]; then
		"${QARAX[@]}" instance-type delete "$PRE_TYPE_NAME" >/dev/null 2>&1 || true
	fi
	cleanup_backup_artifacts >/dev/null 2>&1 || true
	info "Left reusable storage pool '$POOL_NAME' in place."
}
trap cleanup EXIT

command -v jq >/dev/null 2>&1 || die "jq is required"
command -v docker >/dev/null 2>&1 || die "docker is required"

if [[ -z "$(find_qarax_bin)" ]]; then
	step "qarax CLI not found — building it..."
	cargo build -p cli --release --target "$MUSL_TARGET"
fi

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"
QARAX=("$QARAX_BIN" --server "$SERVER")

ensure_stack "$SERVER"
wait_for_api 30

if [[ -z "$HOST_NAME" ]]; then
	HOST_NAME=$("${QARAX[@]}" host list -o json | jq -r '[.[] | select(.status == "up")] | .[0].name // empty')
fi
[[ -n "$HOST_NAME" ]] || die "No UP host found"

banner "Top-level Backups Demo"

step "Using qarax CLI:"
info "$QARAX_BIN"
step "Using host '$HOST_NAME'"
step "Preparing reusable local backup storage at '$POOL_PATH'..."
cleanup_backup_artifacts
ensure_backup_pool
echo

banner "Part 1 — VM backup create/list/get/restore"

step "Creating and starting demo VM..."
run "${QARAX[@]}" vm create --name "$VM_NAME" --vcpus 1 --memory "$MEMORY_BYTES"
VM_CREATED=1
run "${QARAX[@]}" vm start "$VM_NAME"
wait_for_vm_status "$VM_NAME" running 60
info "VM is running."
echo

step "Creating a VM backup through the top-level backup surface..."
VM_BACKUP_JSON=$(
	capture "${QARAX[@]}" backup create vm \
		--vm "$VM_NAME" \
		--name "$VM_BACKUP_NAME" \
		--pool "$POOL_NAME" \
		-o json
)
VM_BACKUP_ID=$(jq -r '.id' <<<"$VM_BACKUP_JSON")
[[ "$(jq -r '.backup_type' <<<"$VM_BACKUP_JSON")" == "vm" ]] || die "VM backup type mismatch"
[[ "$(jq -r '.status' <<<"$VM_BACKUP_JSON")" == "ready" ]] || die "VM backup not ready"
info "VM backup ID: $VM_BACKUP_ID"
echo

step "Listing and inspecting the VM backup..."
run "${QARAX[@]}" backup list --type vm --name "$VM_BACKUP_NAME"
run "${QARAX[@]}" backup get "$VM_BACKUP_NAME"
echo

step "Stopping the VM before restore..."
run "${QARAX[@]}" vm stop "$VM_NAME"
wait_for_vm_status "$VM_NAME" shutdown 60
info "VM is shut down."
echo

step "Restoring the VM from the backup..."
run "${QARAX[@]}" backup restore "$VM_BACKUP_NAME"
wait_for_vm_status "$VM_NAME" running 60
info "VM returned to running state after restore."
echo

step "Cleaning up the demo VM before the database flow..."
run "${QARAX[@]}" vm stop "$VM_NAME"
wait_for_vm_status "$VM_NAME" shutdown 60
run "${QARAX[@]}" vm delete "$VM_NAME"
VM_CREATED=0
echo

banner "Part 2 — Database backup create/list/get/restore"

step "Creating control-plane state that should survive restore..."
run "${QARAX[@]}" instance-type create \
	--name "$PRE_TYPE_NAME" \
	--vcpus 1 \
	--max-vcpus 1 \
	--memory "$MEMORY_BYTES"
PRE_TYPE_CREATED=1
echo

step "Creating a database backup through the top-level backup surface..."
DB_BACKUP_JSON=$(
	capture "${QARAX[@]}" backup create database \
		--name "$DB_BACKUP_NAME" \
		--pool "$POOL_NAME" \
		-o json
)
DB_BACKUP_ID=$(jq -r '.id' <<<"$DB_BACKUP_JSON")
[[ "$(jq -r '.backup_type' <<<"$DB_BACKUP_JSON")" == "database" ]] || die "Database backup type mismatch"
[[ "$(jq -r '.status' <<<"$DB_BACKUP_JSON")" == "ready" ]] || die "Database backup not ready"
info "Database backup ID: $DB_BACKUP_ID"
echo

step "Listing and inspecting the database backup..."
run "${QARAX[@]}" backup list --type database --name "$DB_BACKUP_NAME"
run "${QARAX[@]}" backup get "$DB_BACKUP_NAME"
echo

step "Creating control-plane state that should be rolled back..."
run "${QARAX[@]}" instance-type create \
	--name "$POST_TYPE_NAME" \
	--vcpus 1 \
	--max-vcpus 1 \
	--memory $((512 * 1024 * 1024))
POST_TYPE_CREATED=1
echo

step "Restoring the control-plane database backup..."
run "${QARAX[@]}" backup restore "$DB_BACKUP_NAME"
wait_for_api 30
echo

step "Verifying the restore rewound control-plane state..."
run "${QARAX[@]}" instance-type get "$PRE_TYPE_NAME"
if "${QARAX[@]}" instance-type get "$POST_TYPE_NAME" >/dev/null 2>&1; then
	die "Instance type '$POST_TYPE_NAME' still exists after database restore"
fi
DB_BACKUPS_AFTER=$("${QARAX[@]}" backup list --type database --name "$DB_BACKUP_NAME" -o json)
[[ "$(jq 'length' <<<"$DB_BACKUPS_AFTER")" == "0" ]] || die "Database restore did not rewind backup metadata as expected"
info "Pre-backup data survived; post-backup data was removed."
info "The database backup record itself disappeared after restore, which proves the control-plane DB was rewound."
echo

step "Removing the surviving marker and clearing leftover dump files..."
run "${QARAX[@]}" instance-type delete "$PRE_TYPE_NAME"
PRE_TYPE_CREATED=0
cleanup_backup_artifacts
info "The reusable backup pool remains for future runs."

echo
banner "Demo Complete"
