#!/usr/bin/env bash
#
# Hyperconverged qarax demo — "hosted engine" pattern
#
# Runs the qarax control plane AND qarax-node inside a single Cloud Hypervisor
# VM on bare metal. The host only runs Cloud Hypervisor, passt and a local OCI
# registry; once the VM is up, its own qarax-node (with Cloud Hypervisor,
# overlaybd, etc.) runs all workload VMs via nested KVM.
#
# Network topology:
#   Host (bare metal)
#   ├── passt (vhost-user backend, user-space networking)
#   │   └── Port forwarding: 3000→VM:3000, 8000→VM:8000, 2222→VM:22
#   ├── Local OCI registry (port 5000; the VM reaches it at 192.168.100.1)
#   └── Cloud Hypervisor VM: control-plane
#       ├── eth0 (DHCP via passt, IP 192.168.100.10)
#       ├── qarax API (port 8000 → host port 8000)
#       ├── qarax-node (port 50051)
#       ├── Grafana/otel-lgtm (port 3000 → host port 3000)
#       ├── overlaybd-tcmu
#       └── PostgreSQL (port 5432, local only)
#
# Every run boots a fresh control plane (empty database, no VMs) from a cached
# root disk, so a re-run needs no --cleanup first. The disk is rebuilt only
# when the control-plane image changes.
#
# Prerequisites:
#   - Linux host with KVM + nested KVM (kvm_intel.nested=Y), read/write /dev/kvm
#   - Rust toolchain with the x86_64-unknown-linux-musl target
#   - podman, passt (https://passt.top), guestfish (libguestfs-tools), qemu-img,
#     curl, jq, python3 (for e2e/setup_host.sh)
#
# The control plane runs with APP_ENVIRONMENT=production, which enables API
# token auth. The token is taken from QARAX_TOKEN (see demos/lib.sh) and
# handed to the VM on a small second disk as AUTH_TOKENS.
#
# Usage:
#   ./demos/hyperconverged/run.sh              # Build (cached) + boot + workloads
#   SKIP_BUILD=1 ./demos/hyperconverged/run.sh # Use existing release binaries
#   KEEP=1 ./demos/hyperconverged/run.sh       # On failure, leave the VM running
#   ./demos/hyperconverged/run.sh --cleanup    # Stop the VM and the registry
#   ./demos/hyperconverged/run.sh --with-local # Also create a local storage pool
#   ./demos/hyperconverged/run.sh --with-nfs --nfs-url server:/export  # Also create an NFS pool
#   ./demos/hyperconverged/run.sh --with-local-vm  # Also boot a firmware VM with cloud image
#   ./demos/hyperconverged/run.sh --with-db-vm    # Also boot an OCI PostgreSQL VM
#   ./demos/hyperconverged/run.sh --network-backend bridge # Use bridged VM networking instead of default passt
#   Further options: --db-image IMAGE, --local-pool-path PATH, --cloud-image-url URL
#   Environment: CP_VM_CPUS (default 2), API_HOST_PORT, GRAFANA_HOST_PORT,
#   CP_SSH_PORT, REGISTRY_PORT, DEMO_IMAGE, DB_IMAGE, CLOUD_IMAGE_URL

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEMO_DIR="${REPO_ROOT}/demos/hyperconverged"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

cd "$REPO_ROOT"

# Ensure cargo/rustup are on PATH when running under sudo
if [[ -n "${SUDO_USER:-}" ]]; then
	SUDO_HOME=$(eval echo "~${SUDO_USER}")
	export PATH="${SUDO_HOME}/.cargo/bin:${PATH}"
	export RUSTUP_HOME="${RUSTUP_HOME:-${SUDO_HOME}/.rustup}"
	export CARGO_HOME="${CARGO_HOME:-${SUDO_HOME}/.cargo}"
fi

# Control-plane VM. If Cloud Hypervisor panics with passt vhost-user
# networking and 2+ vCPUs (seen with v51), set CP_VM_CPUS=1.
CP_VM_CPUS="${CP_VM_CPUS:-2}"
CP_VM_MEMORY=$((4 * 1024 * 1024 * 1024)) # 4 GiB in bytes
CP_VM_IP="192.168.100.10"
# passt maps the gateway address to the host's loopback, so the VM also
# reaches the host's registry there.
CP_GW_IP="192.168.100.1"
CP_MAC="52:54:00:aa:bb:01"
CP_ROOTFS_SIZE=8G
CP_IMAGE="qarax-control-plane"
CP_SSH_PORT="${CP_SSH_PORT:-2222}"
GRAFANA_HOST_PORT="${GRAFANA_HOST_PORT:-3000}"
API_HOST_PORT="${API_HOST_PORT:-8000}"
QARAX_NODE_PORT=50051
RUST_HYPERVISOR_FIRMWARE_VERSION="${RUST_HYPERVISOR_FIRMWARE_VERSION:-0.5.0}"
REGISTRY_PORT="${REGISTRY_PORT:-5000}"
REGISTRY_CONTAINER_NAME="qarax-demo-registry-${UID}"
REGISTRY_VOLUME="qarax-demo-registry"
OTEL_LGTM_IMAGE="grafana/otel-lgtm:0.39.0"

# Downloads, the cached root disk, and per-run state (disk overlay, sockets,
# logs). Root (e.g. via sudo) uses /var/cache, so nothing root-owned lands in
# a user's home; other users get their XDG cache dir. Never /tmp: it is often
# tmpfs, and the disk and its overlay take gigabytes.
if ((EUID == 0)); then
	CACHE_DIR="/var/cache/qarax/hyperconverged"
else
	CACHE_DIR="${XDG_CACHE_HOME:-${HOME}/.cache}/qarax/hyperconverged"
fi
RUN_DIR="${CACHE_DIR}/run"
CH_BINARY="${CACHE_DIR}/cloud-hypervisor-${CLOUD_HYPERVISOR_VERSION}"
CH_FIRMWARE="${CACHE_DIR}/hypervisor-fw-${RUST_HYPERVISOR_FIRMWARE_VERSION}"
CP_DISK="${RUN_DIR}/disk.qcow2"
TOKEN_DISK="${RUN_DIR}/token.img"
CP_API_SOCKET="${RUN_DIR}/ch.sock"
CH_PID_FILE="${RUN_DIR}/ch.pid"
CH_LOG="${RUN_DIR}/cloud-hypervisor.log"
CP_CONSOLE_LOG="${RUN_DIR}/console.log"
PASST_SOCKET="${RUN_DIR}/passt.sock"
PASST_LOG="${RUN_DIR}/passt.log"
PASST_PID_FILE="${RUN_DIR}/passt.pid"
REGISTRY_LOG="${RUN_DIR}/registry.log"

# The control plane and Grafana inside the VM, through passt's port forwards.
SERVER="http://127.0.0.1:${API_HOST_PORT}"
GRAFANA_URL="http://127.0.0.1:${GRAFANA_HOST_PORT}"

# Partition layout and boot entry of the root disk: a FAT ESP holding the
# image's kernel and a BLS entry for hypervisor-fw, and an ext4 root filled
# from the image. Part of the disk's cache key, so editing it rebuilds the disk.
ESP_START=2048
ESP_END=$((ESP_START + 256 * 1024 * 1024 / 512 - 1))
DISK_RECIPE=$(
	cat <<EOF
run
part-init /dev/sda gpt
part-add /dev/sda p ${ESP_START} ${ESP_END}
part-add /dev/sda p $((ESP_END + 1)) -34
part-set-gpt-type /dev/sda 1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b
mkfs vfat /dev/sda1
mkfs ext4 /dev/sda2
mount /dev/sda2 /
tar-in - /
mount /dev/sda1 /boot
mkdir-p /boot/loader/entries
glob cp /usr/lib/modules/*/vmlinuz /boot/vmlinuz
write /boot/loader/loader.conf "default qarax\ntimeout 0\n"
write /boot/loader/entries/qarax.conf "title qarax\nlinux /vmlinuz\noptions root=/dev/vda2 rw console=ttyS0 systemd.unified_cgroup_hierarchy=1 net.ifnames=0 biosdevname=0\n"
EOF
)

# ── Helpers ──────────────────────────────────────────────────────────────────

# stop_pid_file FILE MARKER: SIGTERM the process recorded in FILE and wait for
# it to exit, if it is still the one this demo started: its command line holds
# MARKER (one of our socket paths). The cache dir survives reboots, so the PID
# may since belong to an unrelated process.
stop_pid_file() {
	local pid_file="$1" marker="$2" pid
	[[ -f "$pid_file" ]] || return 0
	pid=$(<"$pid_file")
	if [[ -r "/proc/${pid}/cmdline" ]] && grep -qzxF -- "$marker" "/proc/${pid}/cmdline"; then
		kill "$pid"
		wait_until 15 test ! -d "/proc/${pid}" || die "process ${pid} (${marker}) did not exit"
	fi
	rm -f "$pid_file"
}

# Stop the control-plane VM and its passt backend (from an earlier run, or
# this one on failure).
stop_control_plane() {
	stop_pid_file "$CH_PID_FILE" "$CP_API_SOCKET"
	stop_pid_file "$PASST_PID_FILE" "$PASST_SOCKET"
}

cleanup() {
	step "Stopping the hyperconverged demo"
	stop_control_plane
	if podman container exists "$REGISTRY_CONTAINER_NAME"; then
		podman rm -f "$REGISTRY_CONTAINER_NAME" >/dev/null
	fi
	rm -rf "$RUN_DIR"
	ok "Stopped the control-plane VM, passt and the registry"
	info "Kept: downloads and the root disk in ${CACHE_DIR}, registry images in podman volume ${REGISTRY_VOLUME}"
}

# On failure, stop the half-started VM so it doesn't hold the ports; its logs
# stay in RUN_DIR until the next run. KEEP=1 leaves it running for debugging.
on_exit() {
	local rc=$?
	((rc != 0)) || return 0
	warn "Logs: ${RUN_DIR}"
	if [[ "$KEEP" == 1 ]]; then
		warn "KEEP=1: leaving the control-plane VM running (stop it with $0 --cleanup)"
		return 0
	fi
	stop_control_plane
}

# fetch_once URL DEST: download URL into the cache unless it's already there.
# The download lands under a temporary name, so an interrupted one is retried
# instead of being mistaken for a complete file.
fetch_once() {
	[[ -f "$2" ]] && return 0
	info "Downloading $1"
	curl -fsSL "$1" -o "${2}.part"
	mv "${2}.part" "$2"
}

port_in_use() {
	[[ -n "$(ss -tlnH "sport = :$1")" ]]
}

registry_up() {
	curl -sf -o /dev/null --max-time 2 "http://127.0.0.1:${REGISTRY_PORT}/v2/"
}

# registry_has NAME:TAG: whether the demo registry already holds the image.
registry_has() {
	curl -sf -o /dev/null --max-time 5 -I \
		-H "Accept: application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json" \
		"http://127.0.0.1:${REGISTRY_PORT}/v2/${1%:*}/manifests/${1##*:}"
}

# Start (or reuse) the demo's OCI registry and seed it with the otel-lgtm
# image the VM pulls at boot. The registry stores images in a named volume, so
# the seed survives --cleanup and is pushed once, not on every run. Runs in
# the background while the control-plane image builds.
prepare_registry() {
	local state
	if podman container exists "$REGISTRY_CONTAINER_NAME"; then
		state=$(podman inspect -f '{{.State.Status}}' "$REGISTRY_CONTAINER_NAME")
		[[ "$state" == running ]] || podman start "$REGISTRY_CONTAINER_NAME"
	else
		podman run -d --name "$REGISTRY_CONTAINER_NAME" -p "${REGISTRY_PORT}:5000" \
			-v "${REGISTRY_VOLUME}:/var/lib/registry" docker.io/library/registry:2
	fi
	wait_until 30 registry_up || die "registry did not answer on port ${REGISTRY_PORT}"
	if registry_has "$OTEL_LGTM_IMAGE"; then
		echo "${OTEL_LGTM_IMAGE} is already in the registry"
		return 0
	fi
	podman pull "docker.io/${OTEL_LGTM_IMAGE}"
	podman tag "docker.io/${OTEL_LGTM_IMAGE}" "localhost:${REGISTRY_PORT}/${OTEL_LGTM_IMAGE}"
	podman push --tls-verify=false "localhost:${REGISTRY_PORT}/${OTEL_LGTM_IMAGE}"
}

# build_base_disk PATH: stream the image's filesystem straight into a fresh
# raw disk (no intermediate tarball). It's built under a temporary name, so an
# interrupted build is never reused.
build_base_disk() {
	local disk="$1" cid
	rm -f "${CACHE_DIR}"/base-*.raw "${CACHE_DIR}"/base-*.raw.part # earlier images' disks
	truncate -s "$CP_ROOTFS_SIZE" "${disk}.part"
	cid=$(podman create --replace --name qarax-cp-export "$CP_IMAGE" /bin/true)
	podman export "$cid" |
		LIBGUESTFS_BACKEND=direct guestfish --rw --format=raw -a "${disk}.part" \
			-f <(printf '%s\n' "$DISK_RECIPE")
	podman rm "$cid" >/dev/null
	mv "${disk}.part" "$disk"
}

show_cp_logs() {
	local f
	for f in "$CH_LOG" "$PASST_LOG" "$CP_CONSOLE_LOG"; do
		[[ -s "$f" ]] || continue
		echo "--- ${f} (last 30 lines) ---"
		tail -n 30 "$f"
	done >&2
}

# Fail fast, with the VM's logs, if Cloud Hypervisor exited while we wait on
# something inside the VM.
cp_alive() {
	[[ -d "/proc/${CH_PID}" ]] && return 0
	show_cp_logs
	die "the control-plane VM (Cloud Hypervisor) exited"
}

# cp_url_up URL: the VM is alive and URL answers.
cp_url_up() {
	cp_alive && curl -sf -o /dev/null --max-time 2 "$1"
}

grafana_api() {
	curl -fsS -u admin:admin -H 'Content-Type: application/json' "$@"
}

# Import the demo dashboards, pointed at Grafana's Prometheus datasource.
import_dashboards() {
	local prom_uid dashboard
	prom_uid=$(grafana_api "${GRAFANA_URL}/api/datasources" |
		jq -r '[.[] | select(.type == "prometheus")][0].uid // empty')
	if [[ -z "$prom_uid" ]]; then
		warn "Grafana has no Prometheus datasource; skipping the dashboard import"
		return 0
	fi
	grafana_api -d '{"uid": "qarax-demo", "title": "Qarax Demo"}' "${GRAFANA_URL}/api/folders" >/dev/null
	for dashboard in "${DEMO_DIR}"/grafana/*.json; do
		jq --arg uid "$prom_uid" \
			'{dashboard: walk(if . == "__PROM_UID__" then $uid else . end), folderUid: "qarax-demo", overwrite: true}' \
			"$dashboard" | grafana_api -d @- "${GRAFANA_URL}/api/dashboards/db" >/dev/null
		ok "Imported dashboard: $(jq -r .title "$dashboard")"
	done
}

assert_running() {
	local status
	status=$(vm_status "$1")
	[[ "$status" == running ]] || die "VM $1 is '${status}', expected running"
	ok "$1 is running"
}

# ── Arguments ────────────────────────────────────────────────────────────────

WITH_LOCAL=0
WITH_NFS=0
NFS_URL=""
WITH_LOCAL_VM=0
WITH_DB_VM=0
NETWORK_BACKEND="passt"
LOCAL_POOL_PATH="/var/lib/qarax/images"
# Transfers are stored verbatim (no decompression), so this must be a raw or
# qcow2 image, not .xz.
CLOUD_IMAGE_URL="${CLOUD_IMAGE_URL:-https://download.fedoraproject.org/pub/fedora/linux/releases/43/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-43-1.6.x86_64.qcow2}"
DEFAULT_DB_IMAGE="docker.io/library/postgres:17-alpine"
DB_IMAGE="${DB_IMAGE:-${DEFAULT_DB_IMAGE}}"
DEMO_IMAGE="${DEMO_IMAGE:-public.ecr.aws/docker/library/alpine:latest}"
DEMO_VM_NAME="alpine-vm"

usage() {
	echo "Usage: $0 [--cleanup] [--with-local] [--with-nfs --nfs-url HOST:/PATH] [--with-local-vm] [--with-db-vm]"
	echo "          [--network-backend passt|bridge] [--db-image IMAGE] [--local-pool-path PATH] [--cloud-image-url URL]"
	echo "Environment: KEEP=1 (on failure, leave the VM running), SKIP_BUILD=1, CP_VM_CPUS (default 2),"
	echo "             API_HOST_PORT, GRAFANA_HOST_PORT, CP_SSH_PORT, REGISTRY_PORT"
}

# Options that take a value: fail clearly instead of tripping `set -u`.
need_value() {
	[[ $# -ge 2 && -n "$2" ]] || {
		usage
		die "$1 requires a value"
	}
}

while [[ $# -gt 0 ]]; do
	case $1 in
	--cleanup)
		cleanup
		exit 0
		;;
	--with-local)
		WITH_LOCAL=1
		shift
		;;
	--with-nfs)
		WITH_NFS=1
		shift
		;;
	--nfs-url)
		need_value "$@"
		NFS_URL="$2"
		shift 2
		;;
	--with-local-vm)
		WITH_LOCAL_VM=1
		WITH_LOCAL=1 # local VM needs a local pool for the cloud image
		shift
		;;
	--with-db-vm)
		WITH_DB_VM=1
		shift
		;;
	--network-backend)
		need_value "$@"
		NETWORK_BACKEND="$2"
		shift 2
		;;
	--db-image)
		need_value "$@"
		DB_IMAGE="$2"
		shift 2
		;;
	--local-pool-path)
		need_value "$@"
		LOCAL_POOL_PATH="$2"
		shift 2
		;;
	--cloud-image-url)
		need_value "$@"
		CLOUD_IMAGE_URL="$2"
		shift 2
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		usage
		die "Unknown option: $1"
		;;
	esac
done

[[ "$WITH_NFS" -eq 0 || -n "$NFS_URL" ]] || die "--with-nfs requires --nfs-url <server:/export/path>"
[[ "$NETWORK_BACKEND" == "bridge" || "$NETWORK_BACKEND" == "passt" ]] ||
	die "--network-backend must be 'bridge' or 'passt'"

banner "qarax Hyperconverged Demo"

[[ -e /dev/kvm ]] || die "/dev/kvm not found. KVM is required."
[[ -r /dev/kvm && -w /dev/kvm ]] || die "Need read/write access to /dev/kvm (add your user to the kvm group or run via sudo)."
for cmd in podman passt guestfish qemu-img curl jq python3; do
	command -v "$cmd" >/dev/null || die "${cmd} is required (see ${DEMO_DIR}/README.md)"
done
# The token becomes AUTH_TOKENS (comma-separated) in a systemd EnvironmentFile.
[[ "$QARAX_TOKEN" =~ ^[A-Za-z0-9._~+/=-]+$ ]] ||
	die "QARAX_TOKEN must be non-empty and contain no whitespace or commas."

trap on_exit EXIT

# ── Phase 1: clear the previous run, fetch, build ───────────────────────────

step "Clearing any previous run"
stop_control_plane
rm -rf "$RUN_DIR"
mkdir -p "$RUN_DIR"

ports=("$API_HOST_PORT" "$GRAFANA_HOST_PORT" "$CP_SSH_PORT")
# An existing demo registry holds REGISTRY_PORT itself and gets reused.
podman container exists "$REGISTRY_CONTAINER_NAME" || ports+=("$REGISTRY_PORT")
for port in "${ports[@]}"; do
	if port_in_use "$port"; then
		die "Port ${port} is already in use on the host. Stop the service holding it, or override API_HOST_PORT / GRAFANA_HOST_PORT / CP_SSH_PORT / REGISTRY_PORT."
	fi
done
ok "Ports ${ports[*]} are free"

step "Fetching Cloud Hypervisor ${CLOUD_HYPERVISOR_VERSION} and hypervisor-fw ${RUST_HYPERVISOR_FIRMWARE_VERSION}"
fetch_once "https://github.com/cloud-hypervisor/cloud-hypervisor/releases/download/${CLOUD_HYPERVISOR_VERSION}/cloud-hypervisor-static" "$CH_BINARY"
chmod +x "$CH_BINARY"
fetch_once "https://github.com/cloud-hypervisor/rust-hypervisor-firmware/releases/download/${RUST_HYPERVISOR_FIRMWARE_VERSION}/hypervisor-fw" "$CH_FIRMWARE"
ok "Cached in ${CACHE_DIR}"

step "Starting the local OCI registry in the background (log: ${REGISTRY_LOG})"
prepare_registry >"$REGISTRY_LOG" 2>&1 &
REGISTRY_JOB=$!

step "Building qarax, qarax-node and qarax-init (release, musl)"
if [[ -z "${SKIP_BUILD:-}" ]]; then
	[[ "$(rustup target list --installed)" == *"$MUSL_TARGET"* ]] || rustup target add "$MUSL_TARGET"
	cargo build --release -p qarax -p qarax-node -p qarax-init \
		--features "qarax/otel qarax-node/otel"
else
	info "SKIP_BUILD=1: using existing release binaries"
fi
for bin in qarax-server qarax-node qarax-init; do
	[[ -f "${REPO_ROOT}/target/${MUSL_TARGET}/release/${bin}" ]] ||
		die "Binary not found: target/${MUSL_TARGET}/release/${bin} — run without SKIP_BUILD to build first."
done
require_cli

step "Building the control-plane image"
node_artifacts_sha=$(sha256sum versions/node-artifacts.env | cut -d' ' -f1)
podman build \
	--build-arg "CLOUD_HYPERVISOR_VERSION=${CLOUD_HYPERVISOR_VERSION}" \
	--build-arg "NODE_ARTIFACTS_SHA=${node_artifacts_sha}" \
	--build-arg "REGISTRY_PORT=${REGISTRY_PORT}" \
	--build-arg "OTEL_LGTM_IMAGE=${OTEL_LGTM_IMAGE}" \
	-f "${DEMO_DIR}/Containerfile.control-plane" \
	-t "$CP_IMAGE" .

step "Preparing the root disk"
image_id=$(podman image inspect -f '{{.Id}}' "$CP_IMAGE")
disk_key=$(printf '%s\n' "$image_id" "$CP_ROOTFS_SIZE" "$DISK_RECIPE" | sha256sum | cut -c1-16)
BASE_DISK="${CACHE_DIR}/base-${disk_key}.raw"
if [[ -f "$BASE_DISK" ]]; then
	ok "Reusing ${BASE_DISK} (image unchanged)"
else
	info "Image changed: streaming it into a new ${CP_ROOTFS_SIZE} disk"
	build_base_disk "$BASE_DISK"
	ok "Built ${BASE_DISK}"
fi
# Each run boots a copy-on-write overlay of the cached disk, so the control
# plane starts fresh while the disk is built only once per image.
qemu-img create -q -f qcow2 -F raw -b "$BASE_DISK" "$CP_DISK"
# The API token goes on a tiny second disk (the guest's /dev/vdb), which
# qarax.service reads at start: no rebuild or guestfish run per token.
(
	umask 077
	printf 'AUTH_TOKENS=%s\n' "$QARAX_TOKEN" >"$TOKEN_DISK"
)
truncate -s 4K "$TOKEN_DISK"

if ! wait "$REGISTRY_JOB"; then
	cat "$REGISTRY_LOG" >&2
	die "Preparing the local registry failed (log above)"
fi
ok "Registry is up on port ${REGISTRY_PORT} with ${OTEL_LGTM_IMAGE}"

# ── Phase 2: boot the control-plane VM ──────────────────────────────────────

step "Launching the control-plane VM (${CP_VM_CPUS} vCPUs, passt vhost-user networking)"
# passt provides user-space networking for the VM via vhost-user:
#  - no TAP device, no iptables/nftables, no root needed for networking
#  - provides NAT + DHCP for the VM automatically
#  - forwards host ports to the VM via -t
passt --vhost-user \
	-f \
	--socket "$PASST_SOCKET" \
	--address "$CP_VM_IP" \
	--gateway "$CP_GW_IP" \
	-t "${GRAFANA_HOST_PORT}:3000" \
	-t "${API_HOST_PORT}:8000" \
	-t "${CP_SSH_PORT}:22" \
	>"$PASST_LOG" 2>&1 &
echo "$!" >"$PASST_PID_FILE"
wait_until 10 test -S "$PASST_SOCKET" || {
	cat "$PASST_LOG" >&2
	die "passt did not create ${PASST_SOCKET}"
}

# vhost-user networking requires shared guest memory.
"$CH_BINARY" \
	--api-socket "$CP_API_SOCKET" \
	--cpus "boot=${CP_VM_CPUS}" \
	--memory "size=${CP_VM_MEMORY},shared=on" \
	--firmware "$CH_FIRMWARE" \
	--disk "path=${CP_DISK},image_type=qcow2,backing_files=on" \
	"path=${TOKEN_DISK},image_type=raw,readonly=on" \
	--net "vhost_user=true,socket=${PASST_SOCKET},mac=${CP_MAC},num_queues=2,queue_size=256" \
	--serial "file=${CP_CONSOLE_LOG}" \
	--console off \
	>"$CH_LOG" 2>&1 &
CH_PID=$!
echo "$CH_PID" >"$CH_PID_FILE"
info "Console log: ${CP_CONSOLE_LOG}"

step "Waiting for the qarax API in the VM"
wait_until 180 cp_url_up "${SERVER}/" || {
	show_cp_logs
	die "qarax API not reachable at ${SERVER} after 180s"
}
require_server
ok "qarax API is up at ${SERVER}"

step "Registering the control-plane VM as its own compute host"
# qarax host add + host init, retrying while qarax-node starts in the VM.
run bash e2e/setup_host.sh "$SERVER" "$CP_VM_IP" "$QARAX_NODE_PORT" local-node

step "Waiting for Grafana (otel-lgtm) in the VM"
if wait_until 180 cp_url_up "${GRAFANA_URL}/api/health"; then
	ok "Grafana is up at http://localhost:${GRAFANA_HOST_PORT} (admin/admin)"
	import_dashboards
else
	# Telemetry is an add-on: the workloads below don't depend on it.
	warn "Grafana is not up after 180s. Check inside the VM: ssh -p ${CP_SSH_PORT} root@localhost (password: qarax), then journalctl -u otel-lgtm"
fi

# ── Phase 3: storage pools and workload VMs ─────────────────────────────────

step "Creating the default ${NETWORK_BACKEND} network"
run qarax network create --name default --subnet 192.168.100.0/24 --gateway 192.168.100.1 \
	--network-type "$NETWORK_BACKEND"
if [[ "$NETWORK_BACKEND" == "passt" ]]; then
	run qarax network attach-host --network default --host local-node --bridge-name passt0
else
	# Bridged to the VM's eth0
	run qarax network attach-host --network default --host local-node --bridge-name qbr0 --parent-interface eth0
fi

step "Booting an OCI VM from the OverlayBD pool (backed by the host's registry)"
run qarax storage-pool create --name overlaybd-pool --pool-type overlaybd \
	--url "http://${CP_GW_IP}:${REGISTRY_PORT}"
run qarax storage-pool import --pool overlaybd-pool --image-ref "$DEMO_IMAGE" --name alpine-obd
run qarax vm create --name "$DEMO_VM_NAME" --vcpus 1 --memory 256MiB --network default
run qarax vm attach-disk "$DEMO_VM_NAME" --object alpine-obd
run qarax vm start "$DEMO_VM_NAME"
assert_running "$DEMO_VM_NAME"
info "Guest console (last lines):"
qarax vm console "$DEMO_VM_NAME" | tail -n 5 | sed 's/^/    /'

if [[ "$WITH_LOCAL" -eq 1 ]]; then
	step "Creating a local storage pool"
	run qarax storage-pool create --name local-pool --pool-type local \
		--path "$LOCAL_POOL_PATH" --host local-node
fi

if [[ "$WITH_NFS" -eq 1 ]]; then
	step "Creating an NFS storage pool"
	run qarax storage-pool create --name nfs-pool --pool-type nfs --url "$NFS_URL"
fi

if [[ "$WITH_LOCAL_VM" -eq 1 ]]; then
	step "Booting a firmware VM from a cloud image"
	# --wait blocks until the download into the local pool finishes.
	run qarax transfer create --pool local-pool --name cloud-disk \
		--source "$CLOUD_IMAGE_URL" --object-type disk --wait
	run qarax vm create --name cloud-vm --vcpus 1 --memory 1GiB --boot-mode firmware
	run qarax vm attach-disk cloud-vm --object cloud-disk
	run qarax vm start cloud-vm
	assert_running cloud-vm
fi

DB_VM_IP=""
if [[ "$WITH_DB_VM" -eq 1 ]]; then
	step "Booting an OCI PostgreSQL VM"
	if [[ "$DB_IMAGE" == "$DEFAULT_DB_IMAGE" ]]; then
		info "Building a Postgres image with POSTGRES_HOST_AUTH_METHOD=trust into the local registry"
		db_build_dir="${RUN_DIR}/postgres-image"
		mkdir -p "$db_build_dir"
		printf 'FROM %s\nENV POSTGRES_PASSWORD=postgres\nENV POSTGRES_HOST_AUTH_METHOD=trust\n' \
			"$DB_IMAGE" >"${db_build_dir}/Containerfile"
		podman build -t "localhost:${REGISTRY_PORT}/postgres:17-alpine-trust" "$db_build_dir"
		podman push --tls-verify=false "localhost:${REGISTRY_PORT}/postgres:17-alpine-trust"
		DB_IMAGE="${CP_GW_IP}:${REGISTRY_PORT}/postgres:17-alpine-trust"
	fi
	run qarax vm create --name db-vm --vcpus 1 --memory 512MiB --image-ref "$DB_IMAGE" --network default
	run qarax vm start db-vm
	assert_running db-vm
	if [[ "$NETWORK_BACKEND" == "bridge" ]]; then
		db_vm_id=$(qarax vm get db-vm -o json | jq -r .id)
		DB_VM_IP=$(qarax network list-ips default -o json |
			jq -r --arg id "$db_vm_id" '.[] | select(.vm_id == $id) | .ip_address | split("/")[0]')
		[[ -n "$DB_VM_IP" ]] || die "db-vm has no IP allocation on network default"
	fi
fi

# The host's LAN address, for the from-the-LAN URLs. Skipped when the host has
# no default route (`ip route get` would fail).
HOST_LAN_IP=""
if [[ -n "$(ip -4 route show default)" ]]; then
	HOST_LAN_IP=$(ip -4 -j route get 1.1.1.1 | jq -r '.[0].prefsrc // empty')
fi

banner "Hyperconverged qarax Demo Ready"
echo "Control Plane VM (hyperconverged — API + compute):"
echo "  API (this host):  http://localhost:${API_HOST_PORT}/"
if [[ -n "$HOST_LAN_IP" ]]; then
	echo "  API (from LAN):   http://${HOST_LAN_IP}:${API_HOST_PORT}/"
fi
echo "  Swagger UI:       http://localhost:${API_HOST_PORT}/swagger-ui"
echo "  Grafana:          http://localhost:${GRAFANA_HOST_PORT} (admin/admin)"
if [[ -n "$HOST_LAN_IP" ]]; then
	echo "  Grafana (LAN):    http://${HOST_LAN_IP}:${GRAFANA_HOST_PORT}"
fi
echo "  SSH to CP VM:     ssh -o StrictHostKeyChecking=no -p ${CP_SSH_PORT} root@localhost (password: qarax)"
echo "  Console log:      ${CP_CONSOLE_LOG}"
echo ""
echo "Storage pools:"
echo "  overlaybd-pool   (overlaybd, registry: http://${CP_GW_IP}:${REGISTRY_PORT})"
[[ "$WITH_LOCAL" -eq 0 ]] || echo "  local-pool       (local, path: ${LOCAL_POOL_PATH})"
[[ "$WITH_NFS" -eq 0 ]] || echo "  nfs-pool         (nfs, url: ${NFS_URL})"
echo ""
echo "Workload VMs:"
echo "  ${DEMO_VM_NAME}        (OCI: ${DEMO_IMAGE})"
[[ "$WITH_DB_VM" -eq 0 ]] || echo "  db-vm            (OCI: ${DB_IMAGE}, PostgreSQL)"
[[ "$WITH_LOCAL_VM" -eq 0 ]] || echo "  cloud-vm         (firmware boot, cloud image)"
echo ""
echo "Set server and API token for CLI commands:"
echo "  export QARAX_SERVER=http://localhost:${API_HOST_PORT}"
echo "  export QARAX_TOKEN=${QARAX_TOKEN}"
echo ""
echo "Interact with VMs:"
echo "  qarax vm list"
echo "  qarax vm attach ${DEMO_VM_NAME}"
if [[ "$WITH_DB_VM" -eq 1 ]]; then
	echo "  qarax vm attach db-vm             # then, inside the VM: psql -U postgres"
	if [[ "$NETWORK_BACKEND" == "bridge" ]]; then
		echo "  psql -h ${DB_VM_IP} -U postgres  # from the host"
	else
		echo "  (host-side direct DB access is not advertised in passt mode)"
	fi
fi
[[ "$WITH_LOCAL_VM" -eq 0 ]] || echo "  qarax vm attach cloud-vm"
echo ""
echo "Cleanup (as the same user that started the demo; a re-run also replaces the VM):"
echo "  $0 --cleanup"
echo ""
