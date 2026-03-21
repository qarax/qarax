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
#   ├── TAP: qarax-cp-tap0 (192.168.100.1/24)
#   ├── Local OCI registry (port 5000)
#   └── Cloud Hypervisor VM: control-plane
#       ├── eth0 (192.168.100.10/24, gw 192.168.100.1)
#       ├── qarax API (port 8000)
#       ├── qarax-node (port 50051)
#       ├── overlaybd-tcmu
#       └── PostgreSQL (port 5432, local only)
#
# Prerequisites:
#   - Linux host with KVM + nested KVM (kvm_intel.nested=Y)
#   - Rust toolchain (cargo)
#   - podman
#   - cloud-hypervisor binary on PATH (auto-downloaded if missing)
#   - Root/sudo (for TAP device creation and IP configuration)
#   - qarax CLI on PATH
#
# Usage:
#   sudo ./demos/hyperconverged/run.sh             # Full build + run
#   sudo SKIP_BUILD=1 ./demos/hyperconverged/run.sh # Skip cargo build
#   sudo ./demos/hyperconverged/run.sh --cleanup    # Tear down everything
#   sudo ./demos/hyperconverged/run.sh --with-local # Also create a local storage pool
#   sudo ./demos/hyperconverged/run.sh --with-nfs --nfs-url server:/export  # Also create an NFS pool
#   sudo ./demos/hyperconverged/run.sh --with-local-vm  # Also boot a firmware VM with cloud image
#   sudo ./demos/hyperconverged/run.sh --with-db-vm    # Also boot an OCI PostgreSQL VM
#   sudo ./demos/hyperconverged/run.sh --network-backend passt  # Use passt-backed VM networking

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEMO_DIR="$(cd "$(dirname "$0")" && pwd)"
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
CP_VM_CPUS=4
CP_VM_MEMORY=$((4 * 1024 * 1024 * 1024))  # 4 GiB in bytes
CP_VM_IP="192.168.100.10"
HOST_TAP_IP="192.168.100.1"
TAP_NAME="qarax-cp-tap0"
CP_MAC="52:54:00:aa:bb:01"
CP_API_SOCKET="/tmp/qarax-cp.sock"
CP_CONSOLE_LOG="/tmp/qarax-cp-console.log"
CP_ROOTFS="/tmp/qarax-cp-rootfs.img"
CP_ROOTFS_SIZE_MB=4096
CLOUD_HYPERVISOR_VERSION="${CLOUD_HYPERVISOR_VERSION:-v51.0}"
CH_FIRMWARE_VERSION="${CH_FIRMWARE_VERSION:-0.4.2}"
CH_BINARY="${CH_BINARY:-$(command -v cloud-hypervisor 2>/dev/null || echo /usr/local/bin/cloud-hypervisor)}"
CH_FIRMWARE="${REPO_ROOT}/.cache/hypervisor-fw-${CH_FIRMWARE_VERSION}"
QARAX_NODE_PORT=50051
REGISTRY_PORT=5000
REGISTRY_CONTAINER_NAME="qarax-demo-registry"

# ── Cleanup mode ───────────────────────────────────────────────────────────

cleanup() {
	echo -e "${YELLOW}Cleaning up hyperconverged demo...${NC}"

	if [[ -S "$CP_API_SOCKET" ]]; then
		echo "Stopping control plane VM..."
		"$CH_BINARY" --api-socket "$CP_API_SOCKET" api vm.shutdown 2>/dev/null || true
		sleep 1
		"$CH_BINARY" --api-socket "$CP_API_SOCKET" api vmm.shutdown 2>/dev/null || true
		sleep 1
	fi

	if [[ -f /tmp/qarax-cp-ch.pid ]]; then
		kill "$(cat /tmp/qarax-cp-ch.pid)" 2>/dev/null || true
		rm -f /tmp/qarax-cp-ch.pid
	fi

	if podman container exists "$REGISTRY_CONTAINER_NAME" 2>/dev/null; then
		echo "Stopping local registry..."
		podman rm -f "$REGISTRY_CONTAINER_NAME" 2>/dev/null || true
	fi

	iptables -t nat -D POSTROUTING -s 192.168.100.0/24 -j MASQUERADE 2>/dev/null || true

	if ip link show "$TAP_NAME" &>/dev/null; then
		echo "Removing TAP device ${TAP_NAME}..."
		ip link delete "$TAP_NAME" 2>/dev/null || true
	fi

	rm -f "$CP_API_SOCKET" "$CP_CONSOLE_LOG" "$CP_ROOTFS"

	echo -e "${GREEN}Cleanup complete.${NC}"
}

# ── Parse flags ───────────────────────────────────────────────────────────

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
		WITH_LOCAL=1  # local VM needs a local pool for the cloud image
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

NETWORK_BACKEND="${NETWORK_BACKEND:-bridge}"

if [[ "$NETWORK_BACKEND" != "bridge" && "$NETWORK_BACKEND" != "passt" ]]; then
	echo -e "${RED}--network-backend must be 'bridge' or 'passt'${NC}"
	exit 1
fi

# ── Preflight checks ──────────────────────────────────────────────────────

echo "=== qarax Hyperconverged Demo ==="
echo ""

if [[ $EUID -ne 0 ]]; then
	echo -e "${RED}This script must be run as root (needs loop devices, TAP setup, etc.)${NC}"
	echo "  sudo $0 $*"
	exit 1
fi

[[ -e /dev/kvm ]] || die "/dev/kvm not found. KVM is required."
command -v podman &>/dev/null || die "podman is required. Install it and try again."

for tool in sgdisk mkfs.fat partprobe; do
	command -v "$tool" &>/dev/null || die "${tool} is required. Install gdisk, dosfstools, and parted."
done

if [[ ! -x "$CH_BINARY" ]]; then
	echo -e "${YELLOW}cloud-hypervisor not found, downloading ${CLOUD_HYPERVISOR_VERSION}...${NC}"
	CH_BINARY="${REPO_ROOT}/.cache/cloud-hypervisor"
	mkdir -p "$(dirname "$CH_BINARY")"
	curl -fSL \
		"https://github.com/cloud-hypervisor/cloud-hypervisor/releases/download/${CLOUD_HYPERVISOR_VERSION}/cloud-hypervisor-static" \
		-o "$CH_BINARY"
	chmod +x "$CH_BINARY"
	echo -e "${GREEN}Downloaded cloud-hypervisor ${CLOUD_HYPERVISOR_VERSION} to ${CH_BINARY}${NC}"
fi

if [[ ! -f "$CH_FIRMWARE" ]]; then
	echo -e "${YELLOW}Downloading hypervisor-fw v${CH_FIRMWARE_VERSION}...${NC}"
	mkdir -p "$(dirname "$CH_FIRMWARE")"
	curl -fSL \
		"https://github.com/cloud-hypervisor/rust-hypervisor-firmware/releases/download/${CH_FIRMWARE_VERSION}/hypervisor-fw" \
		-o "$CH_FIRMWARE"
	echo -e "${GREEN}Downloaded hypervisor-fw v${CH_FIRMWARE_VERSION} to ${CH_FIRMWARE}${NC}"
fi

# ── Phase 1: Build ─────────────────────────────────────────────────────────

echo -e "${YELLOW}Phase 1: Build${NC}"

if [[ -z "${SKIP_BUILD:-}" ]]; then
	if ! rustup target list --installed 2>/dev/null | grep -q "$MUSL_TARGET"; then
		echo "Adding Rust target ${MUSL_TARGET}..."
		rustup target add "$MUSL_TARGET"
	fi
	echo "Building qarax, qarax-node, and qarax-init..."
	cargo build --release -p qarax -p qarax-node -p qarax-init
else
	echo "Skipping build (SKIP_BUILD=1)"
fi

for bin in qarax qarax-node qarax-init; do
	[[ -f "${REPO_ROOT}/target/${MUSL_TARGET}/release/${bin}" ]] \
		|| die "Binary not found: target/${MUSL_TARGET}/release/${bin} — run without SKIP_BUILD to build first."
done

echo "Building control plane OCI image..."
podman build -f "${DEMO_DIR}/Containerfile.control-plane" -t qarax-control-plane .

echo "Exporting rootfs tarball..."
CONTAINER_ID=$(podman create qarax-control-plane /bin/true | tail -1)
ROOTFS_TAR="/tmp/qarax-cp-rootfs.tar"
rm -f "$ROOTFS_TAR" "$CP_ROOTFS"
podman export "$CONTAINER_ID" -o "$ROOTFS_TAR"
podman rm "$CONTAINER_ID"

echo "Building GPT disk image (ESP + root)..."
ESP_SIZE_MB=256
dd if=/dev/zero of="$CP_ROOTFS" bs=1M count="$CP_ROOTFS_SIZE_MB"

sgdisk -Z "$CP_ROOTFS"
sgdisk -n 1:2048:+${ESP_SIZE_MB}M -t 1:ef00 -c 1:"EFI System" "$CP_ROOTFS"
sgdisk -n 2:0:0 -t 2:8300 -c 2:"Linux root" "$CP_ROOTFS"
sgdisk -p "$CP_ROOTFS"

LOOP_DEV=$(losetup --find --show -P "$CP_ROOTFS")
ESP_DEV="${LOOP_DEV}p1"
ROOT_DEV="${LOOP_DEV}p2"

timeout=5; elapsed=0
while [[ ! -b "$ROOT_DEV" && $elapsed -lt $timeout ]]; do
	sleep 0.5; elapsed=$((elapsed + 1))
done
partprobe "$LOOP_DEV" 2>/dev/null || true

if [[ ! -b "$ESP_DEV" || ! -b "$ROOT_DEV" ]]; then
	echo -e "${RED}Partition devices not found: ${ESP_DEV} / ${ROOT_DEV}${NC}"
	losetup -d "$LOOP_DEV"
	exit 1
fi

mkfs.fat -F 32 "$ESP_DEV"
mkfs.ext4 -F "$ROOT_DEV"

ROOTFS_MOUNT=$(mktemp -d)
mount "$ROOT_DEV" "$ROOTFS_MOUNT"
tar xf "$ROOTFS_TAR" -C "$ROOTFS_MOUNT"

KVER=$(ls "${ROOTFS_MOUNT}/lib/modules/" | sort -V | tail -1)
echo "Kernel version in rootfs: ${KVER}"

VMLINUZ=""
if [[ -f "${ROOTFS_MOUNT}/boot/vmlinuz-${KVER}" ]]; then
	VMLINUZ="${ROOTFS_MOUNT}/boot/vmlinuz-${KVER}"
elif [[ -f "${ROOTFS_MOUNT}/lib/modules/${KVER}/vmlinuz" ]]; then
	VMLINUZ="${ROOTFS_MOUNT}/lib/modules/${KVER}/vmlinuz"
fi

if [[ -z "$VMLINUZ" ]]; then
	echo -e "${RED}Could not find vmlinuz for kernel ${KVER}${NC}"
	echo "Contents of /boot/:" && ls -la "${ROOTFS_MOUNT}/boot/" 2>/dev/null || true
	echo "Contents of /lib/modules/${KVER}/:" && ls -la "${ROOTFS_MOUNT}/lib/modules/${KVER}/" 2>/dev/null || true
	umount "$ROOTFS_MOUNT"
	losetup -d "$LOOP_DEV"
	exit 1
fi
echo "Found vmlinuz at: ${VMLINUZ}"

mount "$ESP_DEV" "${ROOTFS_MOUNT}/boot"
cp "$VMLINUZ" "${ROOTFS_MOUNT}/boot/vmlinuz-${KVER}"
echo "Copied vmlinuz-${KVER} to ESP"

mkdir -p "${ROOTFS_MOUNT}/boot/loader/entries"

cat > "${ROOTFS_MOUNT}/boot/loader/loader.conf" << EOF
default qarax
EOF

cat > "${ROOTFS_MOUNT}/boot/loader/entries/qarax.conf" << EOF
title   qarax Control Plane
linux   /vmlinuz-${KVER}
options root=/dev/vda2 rw console=ttyS0 systemd.unified_cgroup_hierarchy=1 net.ifnames=0 biosdevname=0
EOF

echo "loader.conf:"
cat "${ROOTFS_MOUNT}/boot/loader/loader.conf"
echo "BLS entry:"
cat "${ROOTFS_MOUNT}/boot/loader/entries/qarax.conf"
echo "ESP contents:"
find "${ROOTFS_MOUNT}/boot" -type f

umount "${ROOTFS_MOUNT}/boot"
umount "$ROOTFS_MOUNT"
losetup -d "$LOOP_DEV"
rmdir "$ROOTFS_MOUNT"
rm -f "$ROOTFS_TAR"

echo "Building qarax CLI..."
cargo build --release -p cli
QARAX_CLI="${REPO_ROOT}/target/${MUSL_TARGET}/release/qarax"

echo -e "${GREEN}Phase 1 complete. Rootfs: ${CP_ROOTFS} ($(du -h "$CP_ROOTFS" | cut -f1))${NC}"
echo ""

# ── Phase 2: Start local OCI registry ────────────────────────────────────

echo -e "${YELLOW}Phase 2: Start local OCI registry${NC}"

if podman container exists "$REGISTRY_CONTAINER_NAME" 2>/dev/null; then
	echo "Registry container already exists, reusing"
else
	echo "Starting local registry on port ${REGISTRY_PORT}..."
	podman run -d --name "$REGISTRY_CONTAINER_NAME" \
		-p "${REGISTRY_PORT}:5000" \
		registry:2
fi

echo -n "Waiting for registry"
timeout=15; elapsed=0
while [[ $elapsed -lt $timeout ]]; do
	if curl -sf "http://localhost:${REGISTRY_PORT}/v2/" -o /dev/null 2>/dev/null; then
		echo ""
		echo -e "${GREEN}Local registry is ready on port ${REGISTRY_PORT}${NC}"
		break
	fi
	echo -n "."; sleep 1; elapsed=$((elapsed + 1))
done

if [[ $elapsed -ge $timeout ]]; then
	echo ""
	die "Timeout waiting for local registry"
fi

echo ""

# ── Phase 3: Launch control plane VM ──────────────────────────────────────

echo -e "${YELLOW}Phase 3: Launch control plane VM${NC}"

if ip link show "$TAP_NAME" &>/dev/null; then
	echo "TAP device ${TAP_NAME} already exists, reusing"
else
	echo "Creating TAP device ${TAP_NAME}..."
	ip tuntap add dev "$TAP_NAME" mode tap
fi
ip link set "$TAP_NAME" up
ip addr add "${HOST_TAP_IP}/24" dev "$TAP_NAME" 2>/dev/null || true

sysctl -q net.ipv4.ip_forward=1
if ! iptables -t nat -C POSTROUTING -s 192.168.100.0/24 -j MASQUERADE 2>/dev/null; then
	echo "Setting up NAT masquerade for 192.168.100.0/24..."
	iptables -t nat -A POSTROUTING -s 192.168.100.0/24 -j MASQUERADE
fi

echo "Launching Cloud Hypervisor VM..."
"$CH_BINARY" \
	--api-socket "$CP_API_SOCKET" \
	--cpus boot=${CP_VM_CPUS} \
	--memory size=${CP_VM_MEMORY} \
	--firmware "$CH_FIRMWARE" \
	--disk path="$CP_ROOTFS" \
	--net tap=${TAP_NAME},mac=${CP_MAC} \
	--serial file="$CP_CONSOLE_LOG" \
	--console off &
CH_PID=$!
echo "$CH_PID" > /tmp/qarax-cp-ch.pid

echo "Cloud Hypervisor PID: ${CH_PID}"
echo "Console log: ${CP_CONSOLE_LOG}"
echo ""

echo -n "Waiting for qarax API at http://${CP_VM_IP}:8000/"
timeout=120; elapsed=0
while [[ $elapsed -lt $timeout ]]; do
	if curl -sf "http://${CP_VM_IP}:8000/" -o /dev/null 2>/dev/null; then
		echo ""
		echo -e "${GREEN}qarax API is ready!${NC}"
		break
	fi
	echo -n "."; sleep 2; elapsed=$((elapsed + 2))

	if ! kill -0 "$CH_PID" 2>/dev/null; then
		echo ""
		echo -e "${RED}Cloud Hypervisor process died. Check console log:${NC}"
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

QARAX_API="http://${CP_VM_IP}:8000"

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

# ── Phase 5: Create storage pools and workload VMs ────────────────────────

echo -e "${YELLOW}Phase 5: Create storage pools and workload VMs${NC}"

DEMO_IMAGE="${DEMO_IMAGE:-public.ecr.aws/docker/library/alpine:latest}"
DEMO_VM_NAME="alpine-vm"
DEMO_VM_MEMORY=268435456  # 256 MiB

export QARAX_SERVER="${QARAX_API}"

echo "Creating default network (192.168.100.0/24)..."
"$QARAX_CLI" network create --name default --subnet 192.168.100.0/24 --gateway 192.168.100.1 --network-type "$NETWORK_BACKEND"

echo "Attaching network to host (bridged to eth0)..."
"$QARAX_CLI" network attach-host --network default --host local-node --bridge-name qbr0 --parent-interface eth0
echo ""

echo "Creating overlaybd storage pool..."
"$QARAX_CLI" storage-pool create --name overlaybd-pool --pool-type overlaybd \
	--config '{"url":"http://'"${HOST_TAP_IP}"':'"${REGISTRY_PORT}"'"}'

echo "Attaching overlaybd pool to host..."
"$QARAX_CLI" storage-pool attach-host overlaybd-pool local-node

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
		--config '{"path":"'"${LOCAL_POOL_PATH}"'"}'
	"$QARAX_CLI" storage-pool attach-host local-pool local-node
	echo -e "${GREEN}Local storage pool 'local-pool' created (path: ${LOCAL_POOL_PATH})${NC}"
	echo ""
fi

if [[ "$WITH_NFS" -eq 1 ]]; then
	echo "Creating NFS storage pool..."
	"$QARAX_CLI" storage-pool create --name nfs-pool --pool-type nfs \
		--config '{"url":"'"${NFS_URL}"'"}'
	"$QARAX_CLI" storage-pool attach-host nfs-pool local-node
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
	DB_VM_MEMORY=536870912  # 512 MiB

	if [[ "$DB_IMAGE" == "docker.io/library/postgres:17-alpine" ]]; then
		echo "Building custom Postgres image with POSTGRES_HOST_AUTH_METHOD=trust..."
		cat <<EOF > /tmp/Containerfile.postgres
FROM ${DB_IMAGE}
ENV POSTGRES_PASSWORD=postgres
ENV POSTGRES_HOST_AUTH_METHOD=trust
EOF
		podman build -t localhost:${REGISTRY_PORT}/postgres:17-alpine-trust -f /tmp/Containerfile.postgres
		podman push localhost:${REGISTRY_PORT}/postgres:17-alpine-trust --tls-verify=false
		DB_IMAGE="${HOST_TAP_IP}:${REGISTRY_PORT}/postgres:17-alpine-trust"
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
    print("")' <<< "${DB_VM_JSON}")
	DB_VM_IP=""
	if [[ -n "${DB_VM_ID}" ]]; then
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
        break' "${DB_VM_ID}" <<< "${DB_VM_IPS_JSON}")
	fi

	echo -e "${GREEN}OCI database VM '${DB_VM_NAME}' started (image: ${DB_IMAGE})${NC}"
	echo ""
	echo -e "${YELLOW}Database VM usage:${NC}"
	echo "  qarax vm attach ${DB_VM_NAME}   # interactive console"
	echo "  psql -U postgres                # inside the VM"
	if [[ -n "${DB_VM_IP}" ]]; then
		echo "  psql -h ${DB_VM_IP} -U postgres # from the host"
	fi
	echo ""
fi

# ── Phase 6: Summary ──────────────────────────────────────────────────────

echo -e "${GREEN}=== Hyperconverged qarax Demo Ready ===${NC}"
echo ""
echo "Control Plane VM (hyperconverged — API + compute):"
echo "  API:         http://${CP_VM_IP}:8000/"
echo "  Swagger UI:  http://${CP_VM_IP}:8000/swagger-ui"
echo "  qarax-node:  ${CP_VM_IP}:${QARAX_NODE_PORT}"
echo "  Console log: ${CP_CONSOLE_LOG}"
echo ""
echo "Storage pools:"
echo "  overlaybd-pool   (overlaybd, registry: http://${HOST_TAP_IP}:${REGISTRY_PORT})"
[[ "$WITH_LOCAL" -eq 1 ]] && echo "  local-pool        (local, path: ${LOCAL_POOL_PATH})"
[[ "$WITH_NFS"   -eq 1 ]] && echo "  nfs-pool          (nfs, url: ${NFS_URL})"
echo ""
echo "Workload VMs:"
echo "  ${DEMO_VM_NAME}         (OCI: ${DEMO_IMAGE})"
[[ "$WITH_DB_VM"    -eq 1 ]] && echo "  db-vm             (OCI: ${DB_IMAGE}, PostgreSQL)"
[[ "$WITH_LOCAL_VM" -eq 1 ]] && echo "  cloud-vm          (firmware boot, cloud image)"
echo ""
echo "Set server for CLI commands:"
echo "  export QARAX_SERVER=http://${CP_VM_IP}:8000"
echo ""
echo "Interact with VMs:"
echo "  qarax vm list"
echo "  qarax vm attach ${DEMO_VM_NAME}"
[[ "$WITH_DB_VM"    -eq 1 ]] && echo "  qarax vm attach db-vm"
[[ "$WITH_LOCAL_VM" -eq 1 ]] && echo "  qarax vm attach cloud-vm"
echo ""
echo "Cleanup:"
echo "  sudo ./demos/hyperconverged/run.sh --cleanup"
echo ""
