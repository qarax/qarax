#!/usr/bin/env bash
#
# Hyperconverged qarax demo — "hosted engine" pattern
#
# Runs the qarax control plane AND qarax-node inside a single Cloud Hypervisor
# VM on bare metal. The host only provides a bootstrap qarax-node to launch the
# CP VM; once the VM is up, its internal qarax-node (with Cloud Hypervisor,
# overlaybd, virtiofsd, etc.) handles all workload VMs via nested KVM.
#
# Network topology:
#   Host (bare metal)
#   ├── passt (vhost-user backend, user-space networking)
#   │   ├── UNIX socket /tmp/qarax-cp-passt.sock
#   │   └── Port forwarding: 3000→VM:3000, 8000→VM:8000, 2222→VM:22
#   ├── Local OCI registry (port 5000)
#   └── Cloud Hypervisor VM: control-plane
#       ├── eth0 (DHCP via passt, IP 192.168.100.10)
#       ├── qarax API (port 8000 → host port 8000)
#       ├── qarax-node (port 50051)
#       ├── Grafana/otel-lgtm (port 3000 → host port 3000)
#       ├── overlaybd-tcmu
#       └── PostgreSQL (port 5432, local only)
#
# Prerequisites:
#   - Linux host with KVM + nested KVM (kvm_intel.nested=Y)
#   - Rust toolchain (cargo)
#   - podman
#   - passt (https://passt.top) — user-space networking via vhost-user
#   - cloud-hypervisor binary on PATH (auto-downloaded if missing)
#   - Read/write access to /dev/kvm (for nested virtualization)
#   - qarax CLI on PATH
#
# Usage:
#   ./demos/hyperconverged/run.sh             # Full build + run
#   SKIP_BUILD=1 ./demos/hyperconverged/run.sh # Skip cargo build
#   ./demos/hyperconverged/run.sh --cleanup    # Tear down everything
#   ./demos/hyperconverged/run.sh --with-local # Also create a local storage pool
#   ./demos/hyperconverged/run.sh --with-nfs --nfs-url server:/export  # Also create an NFS pool
#   ./demos/hyperconverged/run.sh --with-local-vm  # Also boot a firmware VM with cloud image
#   ./demos/hyperconverged/run.sh --with-db-vm    # Also boot an OCI PostgreSQL VM
#   ./demos/hyperconverged/run.sh --network-backend bridge # Use bridged VM networking instead of default passt

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEMO_DIR="$(cd "$(dirname "$0")" && pwd)"
CH_VERSION_FILE="${REPO_ROOT}/versions/cloud-hypervisor-version"
source "${REPO_ROOT}/demos/lib.sh"

cd "$REPO_ROOT"

# Ensure cargo/rustup are on PATH when running under sudo
if [[ -n "${SUDO_USER:-}" ]]; then
	SUDO_HOME=$(eval echo "~${SUDO_USER}")
	export PATH="${SUDO_HOME}/.cargo/bin:${PATH}"
	export RUSTUP_HOME="${RUSTUP_HOME:-${SUDO_HOME}/.rustup}"
	export CARGO_HOME="${CARGO_HOME:-${SUDO_HOME}/.cargo}"
fi

# Configuration
CP_VM_CPUS="${CP_VM_CPUS:-4}"
CP_VM_MEMORY=$((4 * 1024 * 1024 * 1024)) # 4 GiB in bytes
CP_VM_IP="192.168.100.10"
CP_GW_IP="192.168.100.1"
CP_MAC="52:54:00:aa:bb:01"
TMP_PREFIX="/tmp/qarax-cp-${UID}"
PASST_SOCKET="${TMP_PREFIX}-passt.sock"
PASST_PID_FILE="${TMP_PREFIX}-passt.pid"
PASST_LOG="${TMP_PREFIX}-passt.log"
CH_PID_FILE="${TMP_PREFIX}-ch.pid"
CP_SSH_PORT="${CP_SSH_PORT:-2222}"
GRAFANA_HOST_PORT="${GRAFANA_HOST_PORT:-3000}"
API_HOST_PORT="${API_HOST_PORT:-8000}"
CP_API_SOCKET="${TMP_PREFIX}.sock"
CP_CONSOLE_LOG="${TMP_PREFIX}-console.log"
CP_ROOTFS="${TMP_PREFIX}-rootfs.img"
CP_KERNEL="${TMP_PREFIX}-vmlinuz"
CH_FIRMWARE="${TMP_PREFIX}-hypervisor-fw"
CP_ROOTFS_SIZE_MB=8192
CLOUD_HYPERVISOR_VERSION="${CLOUD_HYPERVISOR_VERSION:-$(tr -d '\n' <"$CH_VERSION_FILE")}"
CH_BINARY="${CH_BINARY:-$(command -v cloud-hypervisor 2>/dev/null || echo /usr/local/bin/cloud-hypervisor)}"
CH_STABLE_BINARY="${REPO_ROOT}/.cache/cloud-hypervisor-${CLOUD_HYPERVISOR_VERSION}-static"
RUST_HYPERVISOR_FIRMWARE_VERSION="${RUST_HYPERVISOR_FIRMWARE_VERSION:-0.5.0}"
QARAX_NODE_PORT=50051
REGISTRY_PORT="${REGISTRY_PORT:-5000}"
REGISTRY_CONTAINER_NAME="qarax-demo-registry-${UID}"
OTEL_LGTM_CONTAINER_NAME="otel-lgtm"

cleanup() {
	echo -e "${YELLOW}Cleaning up hyperconverged demo...${NC}"

	if [[ -S "$CP_API_SOCKET" ]]; then
		echo "Stopping control plane VM..."
		"$CH_BINARY" --api-socket "$CP_API_SOCKET" api vm.shutdown 2>/dev/null || true
		sleep 1
		"$CH_BINARY" --api-socket "$CP_API_SOCKET" api vmm.shutdown 2>/dev/null || true
		sleep 1
	fi

	if [[ -f "$CH_PID_FILE" ]]; then
		kill "$(cat "$CH_PID_FILE")" 2>/dev/null || true
		rm -f "$CH_PID_FILE"
	fi

	if podman container exists "$REGISTRY_CONTAINER_NAME" 2>/dev/null; then
		echo "Stopping local registry..."
		podman rm -f "$REGISTRY_CONTAINER_NAME" 2>/dev/null || true
	fi

	echo "Stopping passt/Cloud Hypervisor..."
	if [[ -f "$PASST_PID_FILE" ]]; then
		kill "$(cat "$PASST_PID_FILE")" 2>/dev/null || true
		rm -f "$PASST_PID_FILE"
	fi
	pkill -f "passt.*qarax-cp" 2>/dev/null || true
	rm -f "${PASST_SOCKET}" "${PASST_SOCKET}.repair"

	rm -f "$CP_API_SOCKET" "$CP_CONSOLE_LOG" "$CP_ROOTFS" "$CP_KERNEL" "$CH_FIRMWARE" "$PASST_LOG" \
		"${TMP_PREFIX}-loader.conf" "${TMP_PREFIX}-qarax.conf"

	echo -e "${GREEN}Cleanup complete.${NC}"
}

WITH_LOCAL=0
WITH_NFS=0
NFS_URL=""
WITH_LOCAL_VM=0
WITH_DB_VM=0
NETWORK_BACKEND=""
LOCAL_POOL_PATH="/var/lib/qarax/images"
CLOUD_IMAGE_URL="${CLOUD_IMAGE_URL:-https://download.fedoraproject.org/pub/fedora/linux/releases/41/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-41-1.4.x86_64.raw.xz}"
DB_IMAGE="${DB_IMAGE:-docker.io/library/postgres:17-alpine}"

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
		NETWORK_BACKEND="$2"
		shift 2
		;;
	--db-image)
		DB_IMAGE="$2"
		shift 2
		;;
	--local-pool-path)
		LOCAL_POOL_PATH="$2"
		shift 2
		;;
	--cloud-image-url)
		CLOUD_IMAGE_URL="$2"
		shift 2
		;;
	*)
		echo -e "${RED}Unknown option: $1${NC}"
		echo "Usage: $0 [--cleanup] [--with-local] [--with-nfs --nfs-url HOST:/PATH] [--with-local-vm] [--with-db-vm] [--network-backend bridge|passt]"
		exit 1
		;;
	esac
done

if [[ "$WITH_NFS" -eq 1 && -z "$NFS_URL" ]]; then
	echo -e "${RED}--with-nfs requires --nfs-url <server:/export/path>${NC}"
	exit 1
fi

NETWORK_BACKEND="${NETWORK_BACKEND:-passt}"

if [[ "$NETWORK_BACKEND" != "bridge" && "$NETWORK_BACKEND" != "passt" ]]; then
	echo -e "${RED}--network-backend must be 'bridge' or 'passt'${NC}"
	exit 1
fi

CP_LAUNCH_CPUS="$CP_VM_CPUS"
if [[ "$NETWORK_BACKEND" == "passt" && "$CP_VM_CPUS" -gt 1 ]]; then
	echo -e "${YELLOW}Cloud Hypervisor v51 currently panics with passt vhost-user networking, serial logging, and 2+ vCPUs; launching the control-plane VM with 1 vCPU in passt mode. Use --network-backend bridge to keep ${CP_VM_CPUS} vCPUs.${NC}"
	CP_LAUNCH_CPUS=1
fi

echo "=== qarax Hyperconverged Demo ==="
echo ""

[[ -e /dev/kvm ]] || die "/dev/kvm not found. KVM is required."
[[ -r /dev/kvm && -w /dev/kvm ]] || die "Need read/write access to /dev/kvm (add your user to the kvm group or run via sudo)."
command -v podman &>/dev/null || die "podman is required. Install it and try again."
command -v passt &>/dev/null || die "passt is required. Install it (e.g. dnf install passt / apt install passt) and try again."
command -v guestfish &>/dev/null || die "guestfish is required. Install libguestfs-tools and try again."

for _port in "$REGISTRY_PORT" "$GRAFANA_HOST_PORT" "$API_HOST_PORT" "$CP_SSH_PORT"; do
	if ss -tlnH "sport = :${_port}" 2>/dev/null | grep -q .; then
		die "Port ${_port} is already in use on the host. Stop the conflicting service or override the *_PORT env vars before running the demo."
	fi
done

if [[ ! -x "$CH_BINARY" ]]; then
	echo -e "${YELLOW}cloud-hypervisor not found, downloading ${CLOUD_HYPERVISOR_VERSION}...${NC}"
	CH_BINARY="${CH_STABLE_BINARY}"
	mkdir -p "$(dirname "$CH_BINARY")"
	curl -fSL \
		"https://github.com/cloud-hypervisor/cloud-hypervisor/releases/download/${CLOUD_HYPERVISOR_VERSION}/cloud-hypervisor-static" \
		-o "$CH_BINARY"
	chmod +x "$CH_BINARY"
	echo -e "${GREEN}Downloaded cloud-hypervisor ${CLOUD_HYPERVISOR_VERSION} to ${CH_BINARY}${NC}"
fi

if [[ ! -f "$CH_FIRMWARE" ]]; then
	echo -e "${YELLOW}Downloading rust-hypervisor-firmware ${RUST_HYPERVISOR_FIRMWARE_VERSION}...${NC}"
	curl -fSL \
		"https://github.com/cloud-hypervisor/rust-hypervisor-firmware/releases/download/${RUST_HYPERVISOR_FIRMWARE_VERSION}/hypervisor-fw" \
		-o "$CH_FIRMWARE"
	chmod 644 "$CH_FIRMWARE"
fi

probe_api_socket="${TMP_PREFIX}-probe.sock"
probe_cloud_hypervisor_firmware() {
	local ch_bin="$1"
	rm -f "$probe_api_socket"
	set +e
	"$ch_bin" \
		--api-socket "$probe_api_socket" \
		--cpus boot=1 \
		--memory size=$((512 * 1024 * 1024)) \
		--firmware "$CH_FIRMWARE" \
		>/dev/null 2>&1 &
	local probe_pid=$!
	sleep 2
	local probe_rc=0
	if kill -0 "$probe_pid" 2>/dev/null; then
		kill "$probe_pid" 2>/dev/null || true
		wait "$probe_pid" 2>/dev/null || true
	else
		wait "$probe_pid"
		probe_rc=$?
	fi
	set -e
	rm -f "$probe_api_socket"
	[[ "$probe_rc" -ne 139 ]]
}

if ! probe_cloud_hypervisor_firmware "$CH_BINARY"; then
	echo -e "${YELLOW}Selected cloud-hypervisor binary crashes in firmware boot mode; falling back to stable ${CLOUD_HYPERVISOR_VERSION}.${NC}"
	if [[ ! -x "$CH_STABLE_BINARY" ]]; then
		mkdir -p "$(dirname "$CH_STABLE_BINARY")"
		curl -fSL \
			"https://github.com/cloud-hypervisor/cloud-hypervisor/releases/download/${CLOUD_HYPERVISOR_VERSION}/cloud-hypervisor-static" \
			-o "$CH_STABLE_BINARY"
		chmod +x "$CH_STABLE_BINARY"
	fi
	probe_cloud_hypervisor_firmware "$CH_STABLE_BINARY" || die "Stable cloud-hypervisor ${CLOUD_HYPERVISOR_VERSION} also failed firmware probe."
	CH_BINARY="$CH_STABLE_BINARY"
fi

echo -e "${YELLOW}Phase 1: Build${NC}"

if [[ -z "${SKIP_BUILD:-}" ]]; then
	if ! rustup target list --installed 2>/dev/null | grep -q "$MUSL_TARGET"; then
		echo "Adding Rust target ${MUSL_TARGET}..."
		rustup target add "$MUSL_TARGET"
	fi
	echo "Building qarax, qarax-node, and qarax-init..."
	cargo build --release -p qarax -p qarax-node -p qarax-init \
		--features "qarax/otel qarax-node/otel"
else
	echo "Skipping build (SKIP_BUILD=1)"
fi

for bin in qarax qarax-node qarax-init; do
	[[ -f "${REPO_ROOT}/target/${MUSL_TARGET}/release/${bin}" ]] ||
		die "Binary not found: target/${MUSL_TARGET}/release/${bin} — run without SKIP_BUILD to build first."
done

echo "Building control plane OCI image..."
podman build \
	--build-arg "CLOUD_HYPERVISOR_VERSION=${CLOUD_HYPERVISOR_VERSION}" \
	--build-arg "REGISTRY_PORT=${REGISTRY_PORT}" \
	-f "${DEMO_DIR}/Containerfile.control-plane" \
	-t qarax-control-plane .

echo "Exporting rootfs tarball..."
CONTAINER_ID=$(podman create qarax-control-plane /bin/true | tail -1)
ROOTFS_TAR="${TMP_PREFIX}-rootfs.tar"
rm -f "$ROOTFS_TAR" "$CP_ROOTFS" "$CP_KERNEL"
podman export "$CONTAINER_ID" -o "$ROOTFS_TAR"
podman rm "$CONTAINER_ID"

echo "Building root filesystem disk image..."
mapfile -t KERNEL_INFO < <(
	python3 - "$ROOTFS_TAR" "$CP_KERNEL" <<'PY2'
import re
import shutil
import sys
import tarfile

tar_path, kernel_out = sys.argv[1], sys.argv[2]

with tarfile.open(tar_path) as tf:
    members = {member.name.lstrip("./"): member for member in tf.getmembers() if member.isfile()}
    candidates = [
        *sorted((path for path in members if re.fullmatch(r"boot/vmlinuz-.+", path)), reverse=True),
        *sorted((path for path in members if re.fullmatch(r"lib/modules/.+/vmlinuz", path)), reverse=True),
        *sorted((path for path in members if re.fullmatch(r"usr/lib/modules/.+/vmlinuz", path)), reverse=True),
    ]
    for candidate in candidates:
        member = members.get(candidate)
        if member is None:
            continue
        with tf.extractfile(member) as src, open(kernel_out, "wb") as dst:
            shutil.copyfileobj(src, dst)
        print(candidate)
        break
    else:
        raise SystemExit("Could not find a bootable kernel in rootfs tarball")
PY2
)
[[ "${#KERNEL_INFO[@]}" -gt 0 ]] || die "Could not find a bootable kernel in rootfs tarball."
VMLINUX_PATH_IN_TAR="${KERNEL_INFO[0]}"
echo "Using kernel from: ${VMLINUX_PATH_IN_TAR}"

truncate -s "${CP_ROOTFS_SIZE_MB}M" "$CP_ROOTFS"
ESP_START=2048
ESP_SIZE_SECTORS=$((256 * 1024 * 1024 / 512))
ESP_END=$((ESP_START + ESP_SIZE_SECTORS - 1))
ROOT_START=$((ESP_END + 1))
ROOT_END=-34
LOADER_CONF="${TMP_PREFIX}-loader.conf"
BLS_ENTRY="${TMP_PREFIX}-qarax.conf"
cat >"$LOADER_CONF" <<EOF
default qarax
timeout 0
EOF
cat >"$BLS_ENTRY" <<EOF
title qarax
linux /$(basename "$CP_KERNEL")
options root=/dev/vda2 rw console=ttyS0 systemd.unified_cgroup_hierarchy=1 net.ifnames=0 biosdevname=0
EOF

LIBGUESTFS_BACKEND=direct guestfish --rw -a "$CP_ROOTFS" <<EOF
run
part-init /dev/sda gpt
part-add /dev/sda p ${ESP_START} ${ESP_END}
part-add /dev/sda p ${ROOT_START} ${ROOT_END}
part-set-gpt-type /dev/sda 1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b
mkfs vfat /dev/sda1
mkfs ext4 /dev/sda2
mount /dev/sda2 /
tar-in "$ROOTFS_TAR" /
mount /dev/sda1 /boot
mkdir-p /boot/loader
mkdir-p /boot/loader/entries
upload "$CP_KERNEL" /boot/$(basename "$CP_KERNEL")
upload "$LOADER_CONF" /boot/loader/loader.conf
upload "$BLS_ENTRY" /boot/loader/entries/qarax.conf
EOF

rm -f "$ROOTFS_TAR" "$LOADER_CONF" "$BLS_ENTRY"

echo "Building qarax CLI..."
cargo build --release -p cli
QARAX_CLI="${REPO_ROOT}/target/${MUSL_TARGET}/release/qarax"

echo -e "${GREEN}Phase 1 complete. Rootfs: ${CP_ROOTFS} ($(du -h "$CP_ROOTFS" | cut -f1)), kernel: ${CP_KERNEL}${NC}"
echo ""

echo -e "${YELLOW}Phase 2: Start local OCI registry${NC}"

if podman container exists "$REGISTRY_CONTAINER_NAME" 2>/dev/null; then
	registry_state=$(podman inspect "$REGISTRY_CONTAINER_NAME" --format '{{.State.Status}}' 2>/dev/null || true)
	if [[ "$registry_state" == "running" ]]; then
		echo "Registry container already exists, reusing"
	else
		echo "Registry container exists but is not running, starting it..."
		podman start "$REGISTRY_CONTAINER_NAME" >/dev/null
	fi
else
	echo "Starting local registry on port ${REGISTRY_PORT}..."
	podman run -d --name "$REGISTRY_CONTAINER_NAME" \
		-p "${REGISTRY_PORT}:5000" \
		registry:2
fi

echo -n "Waiting for registry"
timeout=15
elapsed=0
while [[ $elapsed -lt $timeout ]]; do
	if curl -sf "http://localhost:${REGISTRY_PORT}/v2/" -o /dev/null 2>/dev/null; then
		echo ""
		echo -e "${GREEN}Local registry is ready on port ${REGISTRY_PORT}${NC}"
		break
	fi
	echo -n "."
	sleep 1
	elapsed=$((elapsed + 1))
done

if [[ $elapsed -ge $timeout ]]; then
	echo ""
	die "Timeout waiting for local registry"
fi

echo "Seeding grafana/otel-lgtm into local registry..."
podman pull docker.io/grafana/otel-lgtm:latest
podman tag docker.io/grafana/otel-lgtm:latest localhost:${REGISTRY_PORT}/grafana/otel-lgtm:latest
podman push localhost:${REGISTRY_PORT}/grafana/otel-lgtm:latest --tls-verify=false
echo -e "${GREEN}grafana/otel-lgtm seeded into local registry${NC}"

echo ""

echo -e "${YELLOW}Phase 3: Launch control plane VM${NC}"

# Kill any stale passt/CH from a previous failed run
if [[ -f "$PASST_PID_FILE" ]]; then
	kill "$(cat "$PASST_PID_FILE")" 2>/dev/null || true
	rm -f "$PASST_PID_FILE"
fi
pkill -f "passt.*qarax-cp" 2>/dev/null || true
rm -f "${PASST_SOCKET}" "${PASST_SOCKET}.repair" "$CP_API_SOCKET"
sleep 0.2

echo "Launching Cloud Hypervisor VM (via passt vhost-user)..."
# passt provides user-space networking for the VM via vhost-user:
#  - no TAP device, no iptables/nftables, no root needed for networking
#  - provides NAT + DHCP for the VM automatically
#  - forwards host ports to the VM via -t

# Start passt in vhost-user mode (background, foreground keeps our PID)
passt --vhost-user \
	-f \
	--socket "$PASST_SOCKET" \
	--address "${CP_VM_IP}" \
	--gateway "${CP_GW_IP}" \
	-t "${GRAFANA_HOST_PORT}:3000" \
	-t "${API_HOST_PORT}:8000" \
	-t "${CP_SSH_PORT}:22" \
	2>"$PASST_LOG" &
PASST_PID=$!
echo "$PASST_PID" >"$PASST_PID_FILE"

# Wait for the vhost-user socket to be ready
echo -n "Waiting for passt socket"
for i in {1..30}; do
	[[ -S "$PASST_SOCKET" ]] && break
	echo -n "."
	sleep 0.2
done
if [[ ! -S "$PASST_SOCKET" ]]; then
	echo ""
	echo -e "${RED}passt socket not ready. Log:${NC}"
	cat "$PASST_LOG" 2>/dev/null || true
	exit 1
fi
echo " ready"

# Determine VM_SEES_HOST from passt log (the address passt NATs to host 127.0.0.1)
sleep 0.3
VM_SEES_HOST=$(awk '/NAT to host/ {for(i=1;i<=NF;i++) if($i ~ /[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+/ && $i !~ /127\./) last=$i} END{print last}' "$PASST_LOG")
if [[ -z "$VM_SEES_HOST" ]]; then
	VM_SEES_HOST="${CP_GW_IP}"
	echo -e "${YELLOW}Could not parse NAT address from passt log; using ${VM_SEES_HOST}${NC}"
fi
echo "VM will reach host registry at ${VM_SEES_HOST}:${REGISTRY_PORT}"

# Start Cloud Hypervisor with vhost-user net (requires shared memory)
"$CH_BINARY" \
	--api-socket "$CP_API_SOCKET" \
	--cpus boot="${CP_LAUNCH_CPUS}" \
	--memory "size=${CP_VM_MEMORY},shared=on" \
	--firmware "$CH_FIRMWARE" \
	--disk "path=$CP_ROOTFS,image_type=raw" \
	--net "vhost_user=true,socket=${PASST_SOCKET},mac=${CP_MAC},num_queues=2,queue_size=256" \
	--serial file="$CP_CONSOLE_LOG" \
	--console off \
	&
CH_PID=$!
echo "$CH_PID" >"$CH_PID_FILE"

echo "passt PID: ${PASST_PID}  Cloud Hypervisor PID: ${CH_PID}"
echo "Console log: ${CP_CONSOLE_LOG}"
echo ""

echo -n "Waiting for qarax API at http://127.0.0.1:${API_HOST_PORT}/"
timeout=120
elapsed=0
while [[ $elapsed -lt $timeout ]]; do
	if curl -sf "http://127.0.0.1:${API_HOST_PORT}/" -o /dev/null 2>/dev/null; then
		echo ""
		echo -e "${GREEN}qarax API is ready!${NC}"
		break
	fi
	echo -n "."
	sleep 2
	elapsed=$((elapsed + 2))

	if ! kill -0 "$CH_PID" 2>/dev/null; then
		echo ""
		echo -e "${RED}Cloud Hypervisor process died. Check logs:${NC}"
		echo "--- passt log ---"
		cat "$PASST_LOG" 2>/dev/null || true
		echo "--- console log ---"
		tail -20 "$CP_CONSOLE_LOG" 2>/dev/null || true
		exit 1
	fi

	if [[ $((elapsed % 20)) -eq 0 ]]; then
		echo -e " (${elapsed}s / ${timeout}s)"
		echo -n "  "
	fi
done

if [[ $elapsed -ge $timeout ]]; then
	echo ""
	echo -e "${RED}Timeout waiting for qarax API. Console log tail:${NC}"
	tail -30 "$CP_CONSOLE_LOG" 2>/dev/null || true
	exit 1
fi

echo ""

echo -e "${YELLOW}Phase 4: Register host${NC}"

QARAX_API="http://127.0.0.1:${API_HOST_PORT}"

echo "Adding CP VM as compute host (self-registration)..."
HOST_ID=$(curl -sf -X POST "${QARAX_API}/hosts" \
	-H "Content-Type: application/json" \
	-d '{
		"name": "local-node",
		"address": "'"${CP_VM_IP}"'",
		"port": '"${QARAX_NODE_PORT}"',
		"host_user": "root",
		"password": ""
	}' | tr -d '"')

[[ -n "$HOST_ID" ]] || die "Failed to add host"
echo -e "Host added: ${HOST_ID}"

echo "Initializing host (gRPC handshake)..."
curl -sf -X POST "${QARAX_API}/hosts/${HOST_ID}/init" | head -c 200
echo ""
echo ""

# Wait for in-VM telemetry backend (Grafana) to be ready
echo -e "${YELLOW}Waiting for Grafana (otel-lgtm) at localhost:${GRAFANA_HOST_PORT}...${NC}"
timeout=120
grafana_ready=0
for ((elapsed = 0; elapsed < timeout; elapsed += 2)); do
	if curl -sf "http://127.0.0.1:${GRAFANA_HOST_PORT}/api/health" -o /dev/null 2>/dev/null; then
		echo -e "${GREEN}Grafana is ready at http://localhost:${GRAFANA_HOST_PORT} (admin/admin)${NC}"
		grafana_ready=1
		break
	fi

	if ! kill -0 "$CH_PID" 2>/dev/null; then
		echo ""
		echo -e "${RED}Cloud Hypervisor process died while waiting for Grafana. Check logs:${NC}"
		echo "--- passt log ---"
		cat "$PASST_LOG" 2>/dev/null || true
		echo "--- console log ---"
		cat "$CP_CONSOLE_LOG" 2>/dev/null || true
		exit 1
	fi

	echo -n "."
	sleep 2
done
if [[ "$grafana_ready" -ne 1 ]]; then
	echo ""
	echo -e "${YELLOW}Warning: Grafana did not become ready within ${timeout}s. It may still be starting inside the control-plane VM.${NC}"
	echo -e "You can check inside the CP VM: ssh -p ${CP_SSH_PORT} root@localhost (password: qarax) and run: podman ps -a"
else
	echo "Importing Qarax demo dashboards into Grafana..."
	python3 - "$GRAFANA_HOST_PORT" \
		"${DEMO_DIR}/grafana/qarax-overview-dashboard.json" \
		"${DEMO_DIR}/grafana/qarax-vm-start-dashboard.json" <<'PY'
import base64
import json
import sys
import urllib.error
import urllib.request

port = sys.argv[1]
dashboard_paths = sys.argv[2:]
base_url = f"http://127.0.0.1:{port}"
auth = base64.b64encode(b"admin:admin").decode()


def request(method, path, payload=None):
    req = urllib.request.Request(f"{base_url}{path}", method=method)
    req.add_header("Authorization", f"Basic {auth}")
    req.add_header("Content-Type", "application/json")
    data = None if payload is None else json.dumps(payload).encode()
    with urllib.request.urlopen(req, data=data, timeout=15) as resp:
        body = resp.read().decode()
        return json.loads(body) if body else {}


datasources = request("GET", "/api/datasources")
prometheus = next((ds for ds in datasources if ds.get("type") == "prometheus"), None)
if prometheus is None:
    print("Warning: Prometheus datasource not found; skipping dashboard import.")
    raise SystemExit(0)

folder_uid = "qarax-demo"
try:
    request("POST", "/api/folders", {"uid": folder_uid, "title": "Qarax Demo"})
except urllib.error.HTTPError as exc:
    if exc.code not in (409, 412):
        raise


def replace_prom_uid(value):
    if isinstance(value, dict):
        return {k: replace_prom_uid(v) for k, v in value.items()}
    if isinstance(value, list):
        return [replace_prom_uid(v) for v in value]
    if value == "__PROM_UID__":
        return prometheus["uid"]
    return value


for dashboard_path in dashboard_paths:
    with open(dashboard_path, encoding="utf-8") as f:
        dashboard = replace_prom_uid(json.load(f))
    request(
        "POST",
        "/api/dashboards/db",
        {"dashboard": dashboard, "folderUid": folder_uid, "overwrite": True},
    )
    print(f"  Imported: {dashboard['title']}")
PY
fi

echo -e "${YELLOW}Phase 5: Create storage pools and workload VMs${NC}"

DEMO_IMAGE="${DEMO_IMAGE:-public.ecr.aws/docker/library/alpine:latest}"
DEMO_VM_NAME="alpine-vm"
DEMO_VM_MEMORY=268435456 # 256 MiB

export QARAX_SERVER="${QARAX_API}"

echo "Creating default ${NETWORK_BACKEND} network (192.168.100.0/24)..."
net_retries=5
net_attempt=0
until "$QARAX_CLI" network create --name default --subnet 192.168.100.0/24 --gateway 192.168.100.1 --network-type "$NETWORK_BACKEND" 2>/dev/null; do
	net_attempt=$((net_attempt + 1))
	if [[ $net_attempt -ge $net_retries ]]; then
		echo -e "${RED}Network creation failed after ${net_retries} attempts.${NC}"
		echo -e "Check qarax logs: ssh -p ${CP_SSH_PORT} root@localhost (password: qarax) and run: journalctl -u qarax -n 50"
		exit 1
	fi
	echo -e "${YELLOW}Network creation failed (attempt ${net_attempt}/${net_retries}), retrying in 5s...${NC}"
	sleep 5
done

if [[ "$NETWORK_BACKEND" == "passt" ]]; then
	echo "Registering passt-backed network on host..."
	"$QARAX_CLI" network attach-host --network default --host local-node --bridge-name passt0
else
	echo "Attaching bridged network to host (bridged to eth0)..."
	"$QARAX_CLI" network attach-host --network default --host local-node --bridge-name qbr0 --parent-interface eth0
fi
echo ""

echo "Creating overlaybd storage pool..."
"$QARAX_CLI" storage-pool create --name overlaybd-pool --pool-type overlaybd \
	--config '{"url":"http://'"${VM_SEES_HOST}"':'"${REGISTRY_PORT}"'"}'

echo "Importing OCI image: ${DEMO_IMAGE}..."
"$QARAX_CLI" storage-pool import --pool overlaybd-pool \
	--image-ref "${DEMO_IMAGE}" \
	--name alpine-obd

echo "Creating OCI VM: ${DEMO_VM_NAME}..."
"$QARAX_CLI" vm create --name "${DEMO_VM_NAME}" --vcpus 1 --memory "${DEMO_VM_MEMORY}" --network default

echo "Attaching OCI disk..."
"$QARAX_CLI" vm attach-disk "${DEMO_VM_NAME}" --object alpine-obd

echo "Starting OCI VM..."
"$QARAX_CLI" vm start "${DEMO_VM_NAME}"
echo ""

if [[ "$WITH_LOCAL" -eq 1 ]]; then
	echo "Creating local storage pool..."
	"$QARAX_CLI" storage-pool create --name local-pool --pool-type local \
		--config '{"path":"'"${LOCAL_POOL_PATH}"'"}' --host local-node
	echo -e "${GREEN}Local storage pool 'local-pool' created (path: ${LOCAL_POOL_PATH})${NC}"
	echo ""
fi

if [[ "$WITH_NFS" -eq 1 ]]; then
	echo "Creating NFS storage pool..."
	"$QARAX_CLI" storage-pool create --name nfs-pool --pool-type nfs \
		--config '{"url":"'"${NFS_URL}"'"}'
	echo -e "${GREEN}NFS storage pool 'nfs-pool' created (url: ${NFS_URL})${NC}"
	echo ""
fi

if [[ "$WITH_LOCAL_VM" -eq 1 ]]; then
	CLOUD_VM_NAME="cloud-vm"

	echo "Transferring cloud image into local pool..."
	"$QARAX_CLI" transfer create --pool local-pool --name cloud-disk \
		--source "$CLOUD_IMAGE_URL" --object-type disk

	echo "Creating firmware-boot VM: ${CLOUD_VM_NAME}..."
	"$QARAX_CLI" vm create --name "${CLOUD_VM_NAME}" --vcpus 1 \
		--memory "${DEMO_VM_MEMORY}" --boot-mode firmware

	echo "Attaching cloud disk..."
	"$QARAX_CLI" vm attach-disk "${CLOUD_VM_NAME}" --object cloud-disk

	echo "Starting firmware-boot VM..."
	"$QARAX_CLI" vm start "${CLOUD_VM_NAME}"

	echo -e "${GREEN}Firmware-boot VM '${CLOUD_VM_NAME}' started${NC}"
	echo ""
fi

if [[ "$WITH_DB_VM" -eq 1 ]]; then
	DB_VM_NAME="db-vm"
	DB_VM_MEMORY=536870912 # 512 MiB

	if [[ "$DB_IMAGE" == "docker.io/library/postgres:17-alpine" ]]; then
		echo "Building custom Postgres image with POSTGRES_HOST_AUTH_METHOD=trust..."
		cat <<EOF >/tmp/Containerfile.postgres
FROM ${DB_IMAGE}
ENV POSTGRES_PASSWORD=postgres
ENV POSTGRES_HOST_AUTH_METHOD=trust
EOF
		podman build -t localhost:${REGISTRY_PORT}/postgres:17-alpine-trust -f /tmp/Containerfile.postgres
		podman push localhost:${REGISTRY_PORT}/postgres:17-alpine-trust --tls-verify=false
		DB_IMAGE="${VM_SEES_HOST}:${REGISTRY_PORT}/postgres:17-alpine-trust"
	fi

	echo "Creating OCI database VM: ${DB_VM_NAME} (image: ${DB_IMAGE})..."
	"$QARAX_CLI" vm create --name "${DB_VM_NAME}" --vcpus 1 --memory "${DB_VM_MEMORY}" \
		--image-ref "${DB_IMAGE}" --network default

	echo "Starting database VM..."
	"$QARAX_CLI" vm start "${DB_VM_NAME}"

	DB_VM_JSON=$("$QARAX_CLI" vm get "${DB_VM_NAME}" --json 2>/dev/null || true)
	DB_VM_ID=$(python3 -c 'import json,sys
try:
    print(json.loads(sys.stdin.read()).get("id",""))
except Exception:
    print("")' <<<"${DB_VM_JSON}")
	DB_VM_IP=""
	if [[ "$NETWORK_BACKEND" == "bridge" && -n "${DB_VM_ID}" ]]; then
		DB_VM_IPS_JSON=$("$QARAX_CLI" network list-ips default --json 2>/dev/null || true)
		DB_VM_IP=$(python3 -c 'import json,sys
vmid=sys.argv[1]
try:
    items=json.loads(sys.stdin.read())
except Exception:
    items=[]
for item in items:
    if item.get("vm_id")==vmid:
        print((item.get("ip_address","") or "").split("/",1)[0])
        break' "${DB_VM_ID}" <<<"${DB_VM_IPS_JSON}")
	fi

	echo -e "${GREEN}OCI database VM '${DB_VM_NAME}' started (image: ${DB_IMAGE})${NC}"
	echo ""
	echo -e "${YELLOW}Database VM usage:${NC}"
	echo "  qarax vm attach ${DB_VM_NAME}   # interactive console"
	echo "  psql -U postgres                # inside the VM"
	if [[ -n "${DB_VM_IP}" ]]; then
		echo "  psql -h ${DB_VM_IP} -U postgres # from the host"
	elif [[ "$NETWORK_BACKEND" == "passt" ]]; then
		echo "  Host-side direct DB access is not advertised in passt mode."
	fi
	echo ""
fi

HOST_LAN_IP="$(
	ip -4 route get 1.1.1.1 2>/dev/null | awk '
		{
			for (i = 1; i <= NF; i++) {
				if ($i == "src") {
					print $(i + 1)
					exit
				}
			}
		}
	'
)"

echo -e "${GREEN}=== Hyperconverged qarax Demo Ready ===${NC}"
echo ""
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
echo "  overlaybd-pool   (overlaybd, registry: http://${VM_SEES_HOST}:${REGISTRY_PORT})"
[[ "$WITH_LOCAL" -eq 1 ]] && echo "  local-pool        (local, path: ${LOCAL_POOL_PATH})"
[[ "$WITH_NFS" -eq 1 ]] && echo "  nfs-pool          (nfs, url: ${NFS_URL})"
echo ""
echo "Workload VMs:"
echo "  ${DEMO_VM_NAME}         (OCI: ${DEMO_IMAGE})"
[[ "$WITH_DB_VM" -eq 1 ]] && echo "  db-vm             (OCI: ${DB_IMAGE}, PostgreSQL)"
[[ "$WITH_LOCAL_VM" -eq 1 ]] && echo "  cloud-vm          (firmware boot, cloud image)"
echo ""
echo "Set server for CLI commands:"
echo "  export QARAX_SERVER=http://localhost:${API_HOST_PORT}"
echo ""
echo "Interact with VMs:"
echo "  qarax vm list"
echo "  qarax vm attach ${DEMO_VM_NAME}"
[[ "$WITH_DB_VM" -eq 1 ]] && echo "  qarax vm attach db-vm"
[[ "$WITH_LOCAL_VM" -eq 1 ]] && echo "  qarax vm attach cloud-vm"
echo ""
echo "Cleanup:"
echo "  sudo ./demos/hyperconverged/run.sh --cleanup"
echo ""
