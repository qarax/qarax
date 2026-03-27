#!/usr/bin/env bash
#
# Demo: upstream 3-node Kubernetes cluster on qarax
#
# Boots a Fedora Cloud image on three firmware VMs using kubeadm.  Each VM
# gets its own disk (a qcow2 clone of the base image) and is configured
# entirely via cloud-init — no custom image build required.
#
# Network layout (10.101.0.0/24):
#   gateway / NAT    10.101.0.1   (qarax-node bridge)
#   k8s-control-0   10.101.0.10
#   k8s-worker-1    10.101.0.11
#   k8s-worker-2    10.101.0.12
#
# Usage:
#   ./demos/k8s-cluster/run.sh
#   ./demos/k8s-cluster/run.sh --cleanup

set -euo pipefail

if [[ "$(id -u)" == "0" ]]; then
	echo "ERROR: Do not run as root — the script calls sudo internally only where needed." >&2
	exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEMO_DIR="$(cd "$(dirname "$0")" && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

KUBERNETES_MINOR="${KUBERNETES_MINOR:-1.32}"
KUBERNETES_VERSION="${KUBERNETES_VERSION:-v${KUBERNETES_MINOR}.13}"
COREDNS_VERSION="${COREDNS_VERSION:-v1.11.3}"
PAUSE_VERSION="${PAUSE_VERSION:-3.10}"
ETCD_VERSION="${ETCD_VERSION:-3.5.24-0}"
FLANNEL_VERSION="${FLANNEL_VERSION:-v0.28.1}"
FLANNEL_CNI_PLUGIN_VERSION="${FLANNEL_CNI_PLUGIN_VERSION:-v1.9.0-flannel1}"
FEDORA_CLOUD_URL="https://download.fedoraproject.org/pub/fedora/linux/releases/43/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-43-1.6.x86_64.qcow2"
DISK_SIZE="20G"

NETWORK_NAME="k8s-net"
SUBNET="10.101.0.0/24"
GATEWAY="10.101.0.1"
BRIDGE_NAME="br-k8s"
VETH_HOST="veth-k8s-host"
HOST_ACCESS_IP="10.101.0.254/24"
CONTROL_IP="10.101.0.10"
WORKER_IPS=("10.101.0.11" "10.101.0.12")
POD_CIDR="10.244.0.0/16"
SMOKE_NODEPORT=30080

HOST_NAME="e2e-node-2"
HOST_ADDRESS="qarax-node-2"
SERVER="http://localhost:8000"
NODE_CONTAINER="e2e-qarax-node-2-1"
STORAGE_POOL_NAME="k8s-pool"
STORAGE_POOL_PATH="/var/lib/qarax/storage/k8s-pool"
FIRMWARE_PATH="/usr/share/cloud-hypervisor/CLOUDHV.fd"

CONTROL_PLANE_VCPUS="${CONTROL_PLANE_VCPUS:-2}"
CONTROL_PLANE_MEMORY_MIB="${CONTROL_PLANE_MEMORY_MIB:-4096}"
WORKER_VCPUS="${WORKER_VCPUS:-2}"
WORKER_MEMORY_MIB="${WORKER_MEMORY_MIB:-3072}"

ARTIFACT_DIR="${TMPDIR:-/tmp}/qarax-k8s-cluster"
KUBECONFIG_PATH="${ARTIFACT_DIR}/admin.conf"
DEBUG_USER="qarax-debug"

NODES=(k8s-control-0 k8s-worker-1 k8s-worker-2)
declare -A NODE_IPS=(
	[k8s-control-0]="${CONTROL_IP}"
	[k8s-worker-1]="${WORKER_IPS[0]}"
	[k8s-worker-2]="${WORKER_IPS[1]}"
)
declare -A NODE_SSH_PORTS=(
	[k8s-control-0]="32210"
	[k8s-worker-1]="32211"
	[k8s-worker-2]="32212"
)

# Runtime state
declare -A NODE_VM_IDS=()
declare -A NODE_DISK_OBJECTS=()
ACCESS_MODE="" # "direct" or "relay"
API_HOST="${CONTROL_IP}"
API_PORT="6443"
CONTAINER_IP="" # Docker IP of NODE_CONTAINER (for relay mode)
KUBECONFIG_RELAY_PORT=38080
SUDO_AVAILABLE=0
DEBUG_SSH_PRIVATE_KEY=""
DEBUG_SSH_PUBLIC_KEY=""
DEBUG_SSH_PUBLIC_KEY_YAML=""
NODE_RESTARTED=0

step() { echo -e "\n${CYAN}=== $* ===${NC}"; }
ok() { echo -e "${GREEN}✓ $*${NC}"; }
info() { echo -e "${YELLOW}$*${NC}"; }

QARAX=() # set after build

run() { "${QARAX[@]}" "$@"; }

wait_for_transfer() {
	local transfer_id="$1"
	local name="$2"
	local max_secs="${3:-600}"
	local elapsed=0
	info "Waiting for transfer '${name}' to complete..."
	while true; do
		local status
		status=$(run --output json transfer list --pool "$STORAGE_POOL_NAME" | python3 -c "
import json,sys
transfers = json.load(sys.stdin)
for t in transfers:
    if t.get('id') == '${transfer_id}':
        print(t.get('status',''))
        break
" 2>/dev/null || true)
		case "$status" in
		completed)
			ok "Transfer '${name}' completed"
			return 0
			;;
		failed) die "Transfer '${name}' failed" ;;
		esac
		[[ $elapsed -ge $max_secs ]] && die "Transfer '${name}' timed out after ${max_secs}s"
		sleep 5
		elapsed=$((elapsed + 5))
	done
}

start_transfer() {
	local name="$1"
	local source="$2"
	local object_type="$3"

	run --output json transfer create \
		--pool "$STORAGE_POOL_NAME" \
		--name "$name" \
		--source "$source" \
		--object-type "$object_type" | python3 -c "
import json,sys
print(json.load(sys.stdin)['id'])
" 2>/dev/null
}

download_base_image_to_node() {
	local temp_path="/tmp/k8s-base-disk.qcow2"
	info "Downloading Fedora base image onto ${NODE_CONTAINER} with curl..." >&2
	docker exec "$NODE_CONTAINER" sh -c "
        curl -L --fail --retry 5 --retry-delay 5 -C - \
            -o '${temp_path}' '${FEDORA_CLOUD_URL}'
    "
	printf '%s\n' "$temp_path"
}

storage_object_id() {
	local name="$1"
	run --output json storage-object list 2>/dev/null | python3 -c "
import json,sys
for o in json.load(sys.stdin):
    if o.get('name') == '${name}':
        print(o['id']); break
" 2>/dev/null || true
}

vm_id() {
	local name="$1"
	run --output json vm get "$name" 2>/dev/null | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])" 2>/dev/null || true
}

setup_direct_access() {
	if ip link show "$VETH_HOST" &>/dev/null; then
		ok "Host veth already set up"
		return
	fi
	local node_pid
	node_pid=$(docker inspect -f '{{.State.Pid}}' "$NODE_CONTAINER")
	local node_br_ip
	node_br_ip=$(docker exec "$NODE_CONTAINER" ip -4 addr show "$BRIDGE_NAME" 2>/dev/null |
		awk '/inet / {print $2; exit}')

	sudo ip link add "$VETH_HOST" type veth peer name "${VETH_HOST}-peer"
	sudo ip link set "${VETH_HOST}-peer" netns "$node_pid"
	sudo nsenter -t "$node_pid" -n -- ip link set "${VETH_HOST}-peer" master "$BRIDGE_NAME"
	sudo nsenter -t "$node_pid" -n -- ip link set "${VETH_HOST}-peer" up
	sudo ip addr add "$HOST_ACCESS_IP" dev "$VETH_HOST"
	sudo ip link set "$VETH_HOST" up
	ok "Host veth ${VETH_HOST} → bridge ${BRIDGE_NAME} in container"
}

teardown_direct_access() {
	ip link show "$VETH_HOST" &>/dev/null || return 0
	if sudo -n true &>/dev/null; then
		sudo ip link del "$VETH_HOST" 2>/dev/null || true
	fi
}

# Open socat relay inside the node container: node listens on HOST_PORT and
# forwards to TARGET_IP:TARGET_PORT. When forwarding to a VM, bind the outbound
# socket to the bridge-side gateway IP so replies return over br-k8s instead of
# trying to reach the container's Docker eth0 address.
start_relay() {
	local name="$1" host_port="$2" target_ip="$3" target_port="$4" source_ip="${5:-}"
	local target_addr="TCP:${target_ip}:${target_port}"
	if [[ -n "$source_ip" ]]; then
		target_addr="${target_addr},bind=${source_ip}"
	fi
	docker exec -d "$NODE_CONTAINER" \
		socat "TCP-LISTEN:${host_port},bind=0.0.0.0,reuseaddr,fork" \
		"$target_addr" 2>/dev/null || true
	info "Relay ${name}: localhost:${host_port} → ${target_ip}:${target_port}"
}

stop_relay() {
	local port="$1"
	docker exec "$NODE_CONTAINER" sh -c \
		"kill \$(fuser ${port}/tcp 2>/dev/null) 2>/dev/null || true"
}

container_file_exists() {
	local path="$1"
	docker exec "$NODE_CONTAINER" test -f "$path" 2>/dev/null
}

rebuild_base_disk_in_place() {
	local tmp_base
	tmp_base=$(download_base_image_to_node)
	docker exec "$NODE_CONTAINER" mkdir -p "$STORAGE_POOL_PATH"
	docker exec "$NODE_CONTAINER" mv -f "$tmp_base" "$BASE_DISK_PATH"
}

rebuild_vm_disk_in_place() {
	local disk_name="$1"
	local disk_size="$2"
	local tmp_disk="/tmp/${disk_name}.qcow2"
	local final_disk="${STORAGE_POOL_PATH}/${disk_name}"
	docker exec "$NODE_CONTAINER" mkdir -p "$STORAGE_POOL_PATH"
	docker exec "$NODE_CONTAINER" sh -c \
		"qemu-img convert -f qcow2 -O qcow2 '${BASE_DISK_PATH}' '${tmp_disk}' && \
         qemu-img resize '${tmp_disk}' '${disk_size}' >/dev/null && \
         mv -f '${tmp_disk}' '${final_disk}'"
}

reconcile_demo_vms_after_node_restart() {
	[[ "$NODE_RESTARTED" -eq 1 ]] || return 0
	step "Removing stale demo VM records after node restart"
	for node in "${NODES[@]}"; do
		if run vm get "$node" &>/dev/null; then
			info "Removing stale VM record for ${node}"
			run vm stop "$node" &>/dev/null || true
			run vm delete "$node" &>/dev/null || true
		fi
	done
	for node in "${NODES[@]}"; do
		! run vm get "$node" &>/dev/null || die "Stale VM record still exists for ${node}; run ./demos/k8s-cluster/run.sh --cleanup and retry"
	done
	ok "Stale demo VMs removed"
}

reset_demo_vms() {
	step "Resetting demo VMs to a clean state"
	for node in "${NODES[@]}"; do
		if run vm get "$node" &>/dev/null; then
			info "Removing existing VM ${node}"
			run vm stop "$node" &>/dev/null || true
			run vm delete "$node" &>/dev/null || true
		fi
	done
	for node in "${NODES[@]}"; do
		! run vm get "$node" &>/dev/null || die "Failed to remove existing VM ${node}"
	done
	ok "Demo VMs removed"
}

prepare_access_mode() {
	if [[ $SUDO_AVAILABLE -eq 1 ]]; then
		ACCESS_MODE="direct"
		setup_direct_access
		ok "Direct host→VM access via ${VETH_HOST}"
	else
		ACCESS_MODE="relay"
		local relay_api_port=36443
		stop_relay "$relay_api_port" 2>/dev/null || true
		stop_relay "$KUBECONFIG_RELAY_PORT" 2>/dev/null || true
		start_relay "k8s-api" "$relay_api_port" "$CONTROL_IP" 6443 "$GATEWAY"
		start_relay "k8s-kubeconfig" "$KUBECONFIG_RELAY_PORT" "$CONTROL_IP" 8080 "$GATEWAY"
		for node in "${NODES[@]}"; do
			start_relay "${node}-ssh" "${NODE_SSH_PORTS[$node]}" "${NODE_IPS[$node]}" 22 "$GATEWAY"
		done
		# Use the container's Docker network IP so the host can reach relay ports
		# that are bound inside the container (not published to the host's localhost).
		CONTAINER_IP=$(docker inspect -f \
			'{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' \
			"$NODE_CONTAINER" 2>/dev/null | head -1)
		API_HOST="${CONTAINER_IP:-localhost}"
		API_PORT="$relay_api_port"
		ok "Relay access: API at ${API_HOST}:${relay_api_port}"
	fi
}

# Generate a random kubeadm bootstrap token (format: [a-z0-9]{6}.[a-z0-9]{16})
generate_token() {
	local chars="abcdefghijklmnopqrstuvwxyz0123456789"
	python3 -c "
import random, string
chars = string.ascii_lowercase + string.digits
part1 = ''.join(random.choices(chars, k=6))
part2 = ''.join(random.choices(chars, k=16))
print(f'{part1}.{part2}')
"
}

ensure_debug_ssh_key() {
	if [[ -n "${K8S_DEMO_DEBUG_SSH_PUBLIC_KEY:-}" ]]; then
		DEBUG_SSH_PUBLIC_KEY="${K8S_DEMO_DEBUG_SSH_PUBLIC_KEY}"
		DEBUG_SSH_PRIVATE_KEY="${K8S_DEMO_DEBUG_SSH_PRIVATE_KEY:-}"
	elif [[ -f "${HOME}/.ssh/id_ed25519.pub" ]]; then
		DEBUG_SSH_PRIVATE_KEY="${HOME}/.ssh/id_ed25519"
		DEBUG_SSH_PUBLIC_KEY="$(<"${DEBUG_SSH_PRIVATE_KEY}.pub")"
	elif [[ -f "${HOME}/.ssh/id_rsa.pub" ]]; then
		DEBUG_SSH_PRIVATE_KEY="${HOME}/.ssh/id_rsa"
		DEBUG_SSH_PUBLIC_KEY="$(<"${DEBUG_SSH_PRIVATE_KEY}.pub")"
	else
		DEBUG_SSH_PRIVATE_KEY="${ARTIFACT_DIR}/id_ed25519"
		ssh-keygen -q -t ed25519 -N "" -C "qarax-k8s-demo" -f "${DEBUG_SSH_PRIVATE_KEY}" >/dev/null
		DEBUG_SSH_PUBLIC_KEY="$(<"${DEBUG_SSH_PRIVATE_KEY}.pub")"
	fi

	[[ -n "${DEBUG_SSH_PUBLIC_KEY}" ]] || die "Failed to determine a debug SSH public key"
	DEBUG_SSH_PUBLIC_KEY_YAML=$(printf '%s' "${DEBUG_SSH_PUBLIC_KEY}" | sed "s/'/''/g")
	if [[ -n "${DEBUG_SSH_PRIVATE_KEY}" ]]; then
		ok "Debug SSH key ready: ${DEBUG_SSH_PRIVATE_KEY}"
	else
		ok "Debug SSH public key injected from environment"
	fi
}

# Build a cloud-init user-data YAML that runs a shell script embedded as
# write_files + runcmd.
render_user_data() {
	local template_path="$1"
	local token="$2"
	local registry_ip="$3"
	local out_path="$4"
	local node_ip="${5:-}"

	# Substitute placeholders in the script, then base64-encode it
	local script_content
	script_content=$(sed \
		-e "s/TOKEN_PLACEHOLDER/${token}/g" \
		-e "s/REGISTRY_IP_PLACEHOLDER/${registry_ip}/g" \
		-e "s/NODE_IP_PLACEHOLDER/${node_ip}/g" \
		-e "s/KUBERNETES_VERSION_PLACEHOLDER/${KUBERNETES_VERSION}/g" \
		-e "s/COREDNS_VERSION_PLACEHOLDER/${COREDNS_VERSION}/g" \
		-e "s/PAUSE_VERSION_PLACEHOLDER/${PAUSE_VERSION}/g" \
		-e "s/ETCD_VERSION_PLACEHOLDER/${ETCD_VERSION}/g" \
		-e "s/FLANNEL_VERSION_PLACEHOLDER/${FLANNEL_VERSION}/g" \
		"$template_path")

	local encoded
	encoded=$(echo "$script_content" | base64 -w 0)

	cat >"$out_path" <<EOF
#cloud-config
ssh_pwauth: false
users:
  - default
  - name: ${DEBUG_USER}
    gecos: Qarax demo debug user
    groups: [wheel]
    sudo: "ALL=(ALL) NOPASSWD:ALL"
    shell: /bin/bash
    lock_passwd: true
    ssh_authorized_keys:
      - '${DEBUG_SSH_PUBLIC_KEY_YAML}'
write_files:
  - path: /usr/local/bin/k8s-setup.sh
    permissions: '0755'
    encoding: b64
    content: ${encoded}
runcmd:
  - /usr/local/bin/k8s-setup.sh
EOF
}

detect_registry_ip() {
	docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' \
		"e2e-registry-1" 2>/dev/null | head -1
}

ensure_registry_relay() {
	local registry_ip="$1"
	stop_relay 5000 2>/dev/null || true
	start_relay "k8s-registry" 5000 "$registry_ip" 5000
	docker exec "$NODE_CONTAINER" sh -c \
		"curl -sf http://${GATEWAY}:5000/v2/ >/dev/null"
	ok "Registry relay available at ${GATEWAY}:5000 for guest VMs"
}

cleanup() {
	step "Cleaning up Kubernetes demo"
	local q
	q="$(find_qarax_bin)"

	teardown_direct_access 2>/dev/null || true

	for port in 36443 38080 36080 5000; do
		stop_relay "$port" 2>/dev/null || true
	done
	for node in "${NODES[@]}"; do
		stop_relay "${NODE_SSH_PORTS[$node]}" 2>/dev/null || true
	done

	if [[ -n "$q" ]]; then
		for node in "${NODES[@]}"; do
			timeout 30 "$q" --server "$SERVER" vm stop "$node" 2>/dev/null || true
			timeout 30 "$q" --server "$SERVER" vm delete "$node" 2>/dev/null || true
		done
		for disk_name in k8s-control-disk k8s-worker-1-disk k8s-worker-2-disk k8s-base-disk; do
			timeout 10 "$q" --server "$SERVER" storage-object delete "$disk_name" 2>/dev/null || true
		done
		timeout 10 "$q" --server "$SERVER" storage-pool delete "$STORAGE_POOL_NAME" 2>/dev/null || true
		timeout 10 "$q" --server "$SERVER" network delete "$NETWORK_NAME" 2>/dev/null || true
	fi

	rm -rf "$ARTIFACT_DIR"
	ok "Artifacts removed"

	cd "$REPO_ROOT/e2e"
	docker compose down -v 2>/dev/null || true
	ok "Stack torn down"
}

if [[ "${1:-}" == "--cleanup" ]]; then
	cleanup
	exit 0
fi

step "Preflight checks"
command -v docker &>/dev/null || die "docker is required"
command -v jq &>/dev/null || die "jq is required"
command -v python3 &>/dev/null || die "python3 is required"
command -v nc &>/dev/null || die "nc (nmap-ncat) is required"
command -v ssh-keygen &>/dev/null || die "ssh-keygen is required"
[[ -e /dev/kvm ]] || die "/dev/kvm not found — KVM is required"
if sudo -n true &>/dev/null; then
	SUDO_AVAILABLE=1
	ok "Passwordless sudo available (will use direct VM access)"
else
	info "No passwordless sudo — will use relay access through ${NODE_CONTAINER}"
fi
ok "Preflight passed"

if [[ -z "$(find_qarax_bin)" ]]; then
	step "Building qarax CLI"
	cargo build -p cli
fi

step "Building qarax binaries (musl)"
rustup target add x86_64-unknown-linux-musl 2>/dev/null || true
cargo build --release -p qarax -p qarax-node -p qarax-init
cargo build -p cli
ok "Binaries ready"

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found"
QARAX=("$QARAX_BIN" --server "$SERVER")

mkdir -p "$ARTIFACT_DIR"

step "Starting qarax stack"
cd "$REPO_ROOT/e2e"
if curl -sf "$SERVER/hosts" -o /dev/null 2>/dev/null; then
	ok "Stack already running"
else
	docker compose up -d --build
	info "Waiting for qarax API..."
	elapsed=0
	until curl -sf "$SERVER/hosts" -o /dev/null 2>/dev/null; do
		sleep 3
		elapsed=$((elapsed + 3))
		[[ $elapsed -gt 120 ]] && die "Timed out waiting for qarax API"
	done
	ok "Stack up"
fi
cd "$REPO_ROOT"

step "Registering qarax-node host"
if ! run host get "$HOST_NAME" &>/dev/null; then
	run host add \
		--name "$HOST_NAME" \
		--address "$HOST_ADDRESS" \
		--port 50051 \
		--user root \
		--password ""
	ok "Host created: ${HOST_NAME}"
else
	ok "Host already exists: ${HOST_NAME}"
fi

for attempt in $(seq 1 10); do
	host_status=$(run --output json host get "$HOST_NAME" | python3 -c \
		"import json,sys; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
	[[ "$host_status" == "up" ]] && break
	if [[ $attempt -le 5 ]]; then
		run host init "$HOST_NAME" &>/dev/null || true
	fi
	sleep 5
done
[[ "$host_status" == "up" ]] || die "Host ${HOST_NAME} did not come up"
ok "Host ${HOST_NAME} is UP"

step "Creating VM network"
if ! run network get "$NETWORK_NAME" &>/dev/null; then
	run network create \
		--name "$NETWORK_NAME" \
		--subnet "$SUBNET" \
		--gateway "$GATEWAY"
	run network attach-host \
		--network "$NETWORK_NAME" \
		--host "$HOST_NAME" \
		--bridge-name "$BRIDGE_NAME"
	ok "Network ${NETWORK_NAME} created"
elif ! docker exec "$NODE_CONTAINER" ip link show "$BRIDGE_NAME" &>/dev/null; then
	info "Bridge ${BRIDGE_NAME} missing (container restarted?), re-attaching..."
	NODE_RESTARTED=1
	run network attach-host \
		--network "$NETWORK_NAME" \
		--host "$HOST_NAME" \
		--bridge-name "$BRIDGE_NAME"
	ok "Network ${NETWORK_NAME} re-attached to host"
else
	ok "Network ${NETWORK_NAME} already exists"
fi

reconcile_demo_vms_after_node_restart

prepare_access_mode

step "Enabling NAT for VM internet access"
docker exec "$NODE_CONTAINER" sh -c '
    echo 1 > /proc/sys/net/ipv4/ip_forward
    iptables -t nat -C POSTROUTING -s 10.101.0.0/24 ! -d 10.101.0.0/24 -j MASQUERADE 2>/dev/null \
        || iptables -t nat -A POSTROUTING -s 10.101.0.0/24 ! -d 10.101.0.0/24 -j MASQUERADE
' || true
ok "NAT masquerade configured"

# Registry access for guest VMs must go through the node-side relay on the VM
# bridge gateway. The registry container's Docker IP is not routable from
# inside the guests.

step "Seeding local registry with demo images"
info "Pulling required images on host and pushing to local registry..."
K8S_IMAGE_LIST=(
	"registry.k8s.io/kube-apiserver:${KUBERNETES_VERSION}"
	"registry.k8s.io/kube-controller-manager:${KUBERNETES_VERSION}"
	"registry.k8s.io/kube-scheduler:${KUBERNETES_VERSION}"
	"registry.k8s.io/kube-proxy:${KUBERNETES_VERSION}"
	"registry.k8s.io/coredns/coredns:${COREDNS_VERSION}"
	"registry.k8s.io/pause:${PAUSE_VERSION}"
	"registry.k8s.io/etcd:${ETCD_VERSION}"
	"ghcr.io/flannel-io/flannel:${FLANNEL_VERSION}"
	"ghcr.io/flannel-io/flannel-cni-plugin:${FLANNEL_CNI_PLUGIN_VERSION}"
	"docker.io/nginxdemos/hello:plain-text"
)
for img in "${K8S_IMAGE_LIST[@]}"; do
	case "$img" in
	registry.k8s.io/*) src_registry="registry.k8s.io" ;;
	ghcr.io/*) src_registry="ghcr.io" ;;
	docker.io/*) src_registry="docker.io" ;;
	*) die "Unknown registry for image ${img}" ;;
	esac
	path="${img#${src_registry}/}"
	local_tag="localhost:5001/${path}"
	if ! docker manifest inspect "$local_tag" &>/dev/null 2>&1; then
		docker pull "$img" 2>&1 | tail -1
		docker tag "$img" "$local_tag"
		docker push "$local_tag" 2>&1 | tail -1
	else
		ok "  ${path} already in local registry"
	fi
done
ok "k8s, flannel, and smoke-test images available in local registry"

step "Creating local storage pool"
if ! run storage-pool get "$STORAGE_POOL_NAME" &>/dev/null; then
	run storage-pool create \
		--name "$STORAGE_POOL_NAME" \
		--pool-type local \
		--config "{\"path\":\"${STORAGE_POOL_PATH}\"}" \
		--attach-all-hosts
	ok "Storage pool ${STORAGE_POOL_NAME} created at ${STORAGE_POOL_PATH}"
else
	ok "Storage pool ${STORAGE_POOL_NAME} already exists"
fi
docker exec "$NODE_CONTAINER" mkdir -p "$STORAGE_POOL_PATH"

BASE_DISK_PATH="${STORAGE_POOL_PATH}/k8s-base-disk"

step "Transferring Fedora Cloud base image"
if ! run storage-object get "k8s-base-disk" &>/dev/null; then
	base_image_tmp=$(download_base_image_to_node)
	transfer_id=$(start_transfer "k8s-base-disk" "$base_image_tmp" disk)
	wait_for_transfer "$transfer_id" "k8s-base-disk" 900
	docker exec "$NODE_CONTAINER" rm -f "$base_image_tmp" 2>/dev/null || true
elif ! container_file_exists "$BASE_DISK_PATH"; then
	info "Base disk record exists but node-local file is missing; rebuilding it in place..."
	rebuild_base_disk_in_place
	ok "Base disk file restored at ${BASE_DISK_PATH}"
else
	ok "Base disk 'k8s-base-disk' already exists"
fi

PREBAKE_HASH=$(
	{
		md5sum "${DEMO_DIR}/prebake.sh"
		printf '%s\n' \
			"${KUBERNETES_MINOR}" \
			"${KUBERNETES_VERSION}" \
			"${COREDNS_VERSION}" \
			"${PAUSE_VERSION}" \
			"${ETCD_VERSION}" \
			"${FLANNEL_VERSION}" \
			"${FLANNEL_CNI_PLUGIN_VERSION}" \
			'offline-images-v2'
	} | md5sum | cut -c1-8
)
BAKE_MARKER="${BASE_DISK_PATH}.k8s-${KUBERNETES_MINOR}-${PREBAKE_HASH}.baked"

step "Pre-installing k8s ${KUBERNETES_MINOR} packages into base image (one-time)"
if docker exec "$NODE_CONTAINER" test -f "$BAKE_MARKER" 2>/dev/null; then
	ok "Base image already has k8s ${KUBERNETES_MINOR} packages baked in"
else
	info "Installing packages into base disk via virt-customize (~10-15 min one-time cost)..."
	local_prebake="${ARTIFACT_DIR}/prebake.sh"
	image_archive="${ARTIFACT_DIR}/k8s-images.tar"
	sed \
		-e "s/KUBERNETES_MINOR_PLACEHOLDER/${KUBERNETES_MINOR}/g" \
		-e "s/PAUSE_VERSION_PLACEHOLDER/${PAUSE_VERSION}/g" \
		"${DEMO_DIR}/prebake.sh" >"$local_prebake"
	info "Saving Kubernetes images for offline guest import..."
	docker image save -o "$image_archive" "${K8S_IMAGE_LIST[@]}"
	docker cp "$local_prebake" "${NODE_CONTAINER}:/tmp/k8s-prebake.sh"
	docker exec "$NODE_CONTAINER" rm -rf /tmp/k8s-image-archives
	docker exec "$NODE_CONTAINER" mkdir -p /tmp/k8s-image-archives
	docker cp "$image_archive" "${NODE_CONTAINER}:/tmp/k8s-image-archives/k8s-images.tar"
	docker exec "$NODE_CONTAINER" chmod +x /tmp/k8s-prebake.sh
	docker exec -e LIBGUESTFS_BACKEND=direct "$NODE_CONTAINER" \
		virt-customize -a "$BASE_DISK_PATH" \
		--copy-in /tmp/k8s-image-archives:/var/lib \
		--run /tmp/k8s-prebake.sh \
		--selinux-relabel
	docker exec "$NODE_CONTAINER" rm -rf /tmp/k8s-image-archives /tmp/k8s-prebake.sh
	docker exec "$NODE_CONTAINER" touch "$BAKE_MARKER"
	ok "k8s ${KUBERNETES_MINOR} packages baked into base image"
fi

reset_demo_vms

step "Creating per-VM disks (qcow2 overlays, resized to ${DISK_SIZE})"
docker exec "$NODE_CONTAINER" mkdir -p "$STORAGE_POOL_PATH"

declare -A DISK_NAMES=(
	[k8s-control-0]="k8s-control-disk"
	[k8s-worker-1]="k8s-worker-1-disk"
	[k8s-worker-2]="k8s-worker-2-disk"
)
declare -A DISK_SIZES=(
	[k8s-control-0]="$DISK_SIZE"
	[k8s-worker-1]="$DISK_SIZE"
	[k8s-worker-2]="$DISK_SIZE"
)

for node in "${NODES[@]}"; do
	disk_name="${DISK_NAMES[$node]}"
	disk_size="${DISK_SIZES[$node]}"
	# Build the overlay in /tmp inside the container so the transfer copy
	# writes to a different destination (pool path) and doesn't truncate the
	# source by copying a file over itself.
	tmp_disk="/tmp/${disk_name}.qcow2"

	if run storage-object get "$disk_name" &>/dev/null; then
		info "Refreshing disk '${disk_name}' from the baked base image..."
		rebuild_vm_disk_in_place "$disk_name" "$disk_size"
		ok "Disk '${disk_name}' ready (${disk_size} overlay)"
		continue
	fi

	# Cloud Hypervisor does not support qcow2 backing files, so convert the
	# base image to a standalone qcow2 (no backing chain) and resize it.
	info "Converting base image for ${disk_name} (this may take a minute)..."
	docker exec "$NODE_CONTAINER" sh -c \
		"qemu-img convert -f qcow2 -O qcow2 '${BASE_DISK_PATH}' '${tmp_disk}' && \
         qemu-img resize '${tmp_disk}' '${disk_size}'"

	transfer_id=$(start_transfer "$disk_name" "$tmp_disk" disk)
	wait_for_transfer "$transfer_id" "$disk_name" 120

	ok "Disk '${disk_name}' ready (${disk_size} overlay)"
done

step "Generating cloud-init user-data"
KUBEADM_TOKEN=$(generate_token)
info "Bootstrap token: ${KUBEADM_TOKEN}"
ensure_debug_ssh_key

REGISTRY_IP=$(detect_registry_ip)
[[ -n "$REGISTRY_IP" ]] || die "Could not detect registry container IP — is e2e-registry-1 running?"
info "Registry container IP: ${REGISTRY_IP}"
ensure_registry_relay "$REGISTRY_IP"
GUEST_REGISTRY_IP="$GATEWAY"

CONTROL_USERDATA="${ARTIFACT_DIR}/user-data-control.yaml"
declare -A USERDATA_PATHS=(
	[k8s-control-0]="${CONTROL_USERDATA}"
	[k8s-worker-1]="${ARTIFACT_DIR}/user-data-k8s-worker-1.yaml"
	[k8s-worker-2]="${ARTIFACT_DIR}/user-data-k8s-worker-2.yaml"
)

render_user_data "${DEMO_DIR}/cloud-init-control.sh" "$KUBEADM_TOKEN" "$GUEST_REGISTRY_IP" "$CONTROL_USERDATA"
render_user_data "${DEMO_DIR}/cloud-init-worker.sh" "$KUBEADM_TOKEN" "$GUEST_REGISTRY_IP" \
	"${USERDATA_PATHS[k8s-worker-1]}" "${NODE_IPS[k8s-worker-1]}"
render_user_data "${DEMO_DIR}/cloud-init-worker.sh" "$KUBEADM_TOKEN" "$GUEST_REGISTRY_IP" \
	"${USERDATA_PATHS[k8s-worker-2]}" "${NODE_IPS[k8s-worker-2]}"
ok "cloud-init files written to ${ARTIFACT_DIR}"

step "Creating k8s VMs (firmware boot)"
CONTROL_MEM=$((CONTROL_PLANE_MEMORY_MIB * 1024 * 1024))
WORKER_MEM=$((WORKER_MEMORY_MIB * 1024 * 1024))

for node in "${NODES[@]}"; do
	if run vm get "$node" &>/dev/null; then
		ok "VM '${node}' already exists"
		NODE_VM_IDS[$node]=$(vm_id "$node")
		continue
	fi

	disk_name="${DISK_NAMES[$node]}"
	[[ "$node" == "k8s-control-0" ]] && vcpus=$CONTROL_PLANE_VCPUS mem=$CONTROL_MEM || {
		vcpus=$WORKER_VCPUS
		mem=$WORKER_MEM
	}
	udata="${USERDATA_PATHS[$node]}"

	# Static IP is configured by the runcmd script (ip/iproute2) rather than
	# cloud-init network-config, which is unreliable on Fedora 43+NM.
	run vm create \
		--name "$node" \
		--boot-mode firmware \
		--vcpus "$vcpus" \
		--memory "$mem" \
		--network "$NETWORK_NAME" \
		--ip "${NODE_IPS[$node]}" \
		--offload-tso false \
		--offload-ufo false \
		--offload-csum false \
		--root-disk "$disk_name" \
		--cloud-init-user-data "$udata"

	NODE_VM_IDS[$node]=$(vm_id "$node")
	ok "VM '${node}' created (id=${NODE_VM_IDS[$node]})"
done

step "Starting VMs"
for node in "${NODES[@]}"; do
	status=$(run --output json vm get "$node" | python3 -c \
		"import json,sys; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
	if [[ "$status" == "running" ]]; then
		ok "VM '${node}' already running"
	else
		run vm start "$node"
		ok "VM '${node}' started"
	fi
done

step "Waiting for Kubernetes API server"
info "This takes 5-15 minutes (images pre-pulled from local registry)..."
info "Watch progress: docker exec ${NODE_CONTAINER} tail -f /dev/null  (use 'vm attach' in another shell)"
elapsed=0
max_wait=3600
until curl -sk --connect-timeout 3 --max-time 5 "https://${API_HOST}:${API_PORT}/healthz" 2>/dev/null | grep -q ok; do
	sleep 15
	elapsed=$((elapsed + 15))
	[[ $elapsed -ge $max_wait ]] && die "Timed out waiting for k8s API (${max_wait}s)"
	info "  ${elapsed}s elapsed, API not ready yet..."
done
ok "Kubernetes API server is up (${elapsed}s)"

# ── Fetch kubeconfig ──────────────────────────────────────────────────────────

step "Fetching kubeconfig"
info "Waiting for control-plane HTTP server to serve kubeconfig..."
elapsed=0
kc_host="${API_HOST}"
kc_port="${KUBECONFIG_RELAY_PORT:-8080}"
[[ "$ACCESS_MODE" == "direct" ]] && kc_host="${CONTROL_IP}" && kc_port=8080
until curl -sf --connect-timeout 3 --max-time 5 "http://${kc_host}:${kc_port}/k8s-admin.conf" -o "$KUBECONFIG_PATH" 2>/dev/null; do
	sleep 10
	elapsed=$((elapsed + 10))
	[[ $elapsed -ge 300 ]] && die "Timed out waiting for kubeconfig (300s)"
done

if [[ "$ACCESS_MODE" == "relay" ]]; then
	# Rewrite server URL to use the relay host
	python3 -c "
import sys, re
conf = open('${KUBECONFIG_PATH}').read()
conf = re.sub(r'server: https://[^:]+:6443', 'server: https://${API_HOST}:${API_PORT}', conf)
if re.search(r'^\s*tls-server-name:', conf, re.M):
    conf = re.sub(r'^\s*tls-server-name:.*$', '    tls-server-name: ${CONTROL_IP}', conf, flags=re.M)
else:
    conf = re.sub(r'(^\s*server: https://[^\n]+$)', r'\1\n    tls-server-name: ${CONTROL_IP}', conf, flags=re.M)
open('${KUBECONFIG_PATH}', 'w').write(conf)
"
fi
ok "Kubeconfig saved to ${KUBECONFIG_PATH}"

export KUBECONFIG="$KUBECONFIG_PATH"
kubectl="kubectl --kubeconfig=${KUBECONFIG_PATH}"

step "Waiting for all nodes to be Ready"
info "Workers are installing Kubernetes and joining the cluster..."
elapsed=0
max_wait=3600
while true; do
	ready_count=$($kubectl get nodes --no-headers 2>/dev/null |
		grep -c " Ready " || true)
	[[ "$ready_count" -ge 3 ]] && break
	sleep 20
	elapsed=$((elapsed + 20))
	[[ $elapsed -ge $max_wait ]] && die "Timed out waiting for 3 Ready nodes (${max_wait}s)"
	info "  ${ready_count}/3 nodes Ready (${elapsed}s elapsed)..."
done
ok "All 3 nodes are Ready"
$kubectl get nodes

$kubectl create namespace demo 2>/dev/null || true
$kubectl apply -n demo -f "${DEMO_DIR}/smoke.yaml"
$kubectl -n demo rollout status deployment/hello-k8s --timeout=10m
$kubectl -n demo wait --for=condition=ready pod -l app=hello-k8s --timeout=5m

smoke_host="${CONTROL_IP}"
smoke_port="${SMOKE_NODEPORT}"
if [[ "$ACCESS_MODE" == "relay" ]]; then
	stop_relay 36080 2>/dev/null || true
	start_relay "k8s-smoke" 36080 "${CONTROL_IP}" "${SMOKE_NODEPORT}" "$GATEWAY"
	smoke_host="${CONTAINER_IP:-localhost}"
	smoke_port=36080
	sleep 2
fi

echo ""
info "Sending request to smoke workload at http://${smoke_host}:${smoke_port}/..."
response=$(curl -sf "http://${smoke_host}:${smoke_port}/" 2>/dev/null || echo "")
if [[ -n "$response" ]]; then
	ok "Smoke workload responded: ${response}"
else
	info "No HTTP response (workload may not expose HTTP on NodePort — pod is running)"
fi

echo ""
echo -e "${GREEN}${BOLD}══════════════════════════════════════════════════${NC}"
echo -e "${GREEN}${BOLD}  3-node Kubernetes cluster is up and running!    ${NC}"
echo -e "${GREEN}${BOLD}══════════════════════════════════════════════════${NC}"
echo ""
echo -e "  Kubeconfig:  ${KUBECONFIG_PATH}"
echo -e "  API server:  https://${API_HOST}:${API_PORT}"
echo -e "  Debug user:  ${DEBUG_USER}"
if [[ -n "${DEBUG_SSH_PRIVATE_KEY}" ]]; then
	ssh_identity="-i ${DEBUG_SSH_PRIVATE_KEY} "
else
	ssh_identity=""
fi
for node in "${NODES[@]}"; do
	if [[ "$ACCESS_MODE" == "relay" ]]; then
		echo -e "  ${node} SSH: ssh ${ssh_identity}-o StrictHostKeyChecking=no -p ${NODE_SSH_PORTS[$node]} ${DEBUG_USER}@${CONTAINER_IP:-localhost}"
	else
		echo -e "  ${node} SSH: ssh ${ssh_identity}-o StrictHostKeyChecking=no ${DEBUG_USER}@${NODE_IPS[$node]}"
	fi
done
echo ""
echo -e "  kubectl --kubeconfig=${KUBECONFIG_PATH} get nodes"
echo ""
echo -e "  To tear down: ${CYAN}./demos/k8s-cluster/run.sh --cleanup${NC}"
