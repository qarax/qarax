#!/usr/bin/env bash
#
# Demo: upstream 3-node Kubernetes cluster on qarax
#
# Boots the Fedora Cloud image on three firmware (UEFI) VMs and turns them
# into a kubeadm cluster with Flannel, configured entirely through cloud-init.
# The Kubernetes packages and images are baked into a base image once per
# Kubernetes minor and cached; each run copies it into three fresh disks.
#
# The VMs run on the local stack's second node container (qarax-node-2), so
# the cluster can keep running without taking memory from the default node.
#
# Network layout (qarax network demo-k8s-net, 10.101.0.0/24):
#   gateway / NAT        10.101.0.1    (bridge in the qarax-node-2 container)
#   demo-k8s-control-0   10.101.0.10
#   demo-k8s-worker-1    10.101.0.11
#   demo-k8s-worker-2    10.101.0.12
#
# Usage:
#   ./demos/k8s-cluster/run.sh            # (re)create the cluster
#   ./demos/k8s-cluster/run.sh --cleanup  # delete the demo's VMs, disks, network and pool
#   ./demos/k8s-cluster/run.sh --purge    # --cleanup, plus the cached base image and SSH key

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEMO_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

# ── Configuration ────────────────────────────────────────────────────────────

KUBERNETES_MINOR="${KUBERNETES_MINOR:-1.35}"
FLANNEL_VERSION="${FLANNEL_VERSION:-v0.28.1}"
# demos/hyperconverged downloads the same image.
FEDORA_CLOUD_URL="https://download.fedoraproject.org/pub/fedora/linux/releases/43/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-43-1.6.x86_64.qcow2"
DISK_SIZE="20GiB"
CONTROL_PLANE_VCPUS="${CONTROL_PLANE_VCPUS:-2}"
CONTROL_PLANE_MEMORY_MIB="${CONTROL_PLANE_MEMORY_MIB:-4096}"
WORKER_VCPUS="${WORKER_VCPUS:-2}"
WORKER_MEMORY_MIB="${WORKER_MEMORY_MIB:-3072}"
SETUP_TIMEOUT=1200

PREFIX="demo-k8s-"
CONTROL_VM="${PREFIX}control-0"
VMS=("$CONTROL_VM" "${PREFIX}worker-1" "${PREFIX}worker-2")
declare -A VM_IPS=(
	["demo-k8s-control-0"]="10.101.0.10"
	["demo-k8s-worker-1"]="10.101.0.11"
	["demo-k8s-worker-2"]="10.101.0.12"
)
CONTROL_IP="${VM_IPS[$CONTROL_VM]}"
NETWORK_NAME="${PREFIX}net"
SUBNET="10.101.0.0/24"
GATEWAY="10.101.0.1"
BRIDGE_NAME="br-demo-k8s"
POOL_NAME="${PREFIX}pool"
POOL_PATH="/var/lib/qarax/storage/${POOL_NAME}"
SMOKE_NODEPORT=30080

# Host access to the VM network: a veth into the node's bridge (needs
# passwordless sudo), or socat relays inside the node container.
VETH_HOST="veth-k8s"
VETH_PEER="veth-k8s-br"
HOST_ACCESS_IP="10.101.0.254/24"
API_RELAY_PORT=36443
SMOKE_RELAY_PORT=36080
declare -A SSH_RELAY_PORTS=(
	["demo-k8s-control-0"]="32210"
	["demo-k8s-worker-1"]="32211"
	["demo-k8s-worker-2"]="32212"
)

NODE_SERVICE="qarax-node-2"
NODE_CACHE_DIR="/var/lib/qarax/storage/${PREFIX}cache"
DEMO_HOME="${XDG_CACHE_HOME:-$HOME/.cache}/qarax/k8s-cluster"
KUBECONFIG_PATH="${DEMO_HOME}/admin.conf"
SSH_KEY="${DEMO_HOME}/id_ed25519"
DEBUG_USER="qarax-debug"
SSH_OPTS=(-i "$SSH_KEY" -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=5
	-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)

# The baked base image is named after everything that goes into it. It lives
# in the node container (qemu-img reads it there) with a copy in DEMO_HOME,
# which survives tearing the stack down.
BAKE_KEY=$({
	cat "${DEMO_DIR}/prebake.sh"
	echo "${FEDORA_CLOUD_URL} ${KUBERNETES_MINOR} ${FLANNEL_VERSION}"
} | sha256sum | cut -c1-12)
BASE_NAME="base-k8s-${KUBERNETES_MINOR}-${BAKE_KEY}.qcow2"
BASE_IMAGE="${NODE_CACHE_DIR}/${BASE_NAME}"
BASE_CACHE="${DEMO_HOME}/${BASE_NAME}"

# Set once the node is known (select_node) and access is set up (prepare_access).
HOST_NAME=""
NODE_CID=""
ACCESS_MODE=""
API_URL=""
SMOKE_URL=""
declare -A SSH_HOSTS=()
declare -A SSH_PORTS=()

usage() {
	cat <<EOF
Usage: $0 [--cleanup | --purge | --help]

Creates a 3-node kubeadm cluster on qarax and leaves it running.

  --cleanup   delete the demo's VMs, disks, network and storage pool
  --purge     --cleanup, plus the cached base image and SSH key

Environment:
  KUBERNETES_MINOR          Kubernetes minor to install (default ${KUBERNETES_MINOR})
  FLANNEL_VERSION           Flannel release (default ${FLANNEL_VERSION})
  CONTROL_PLANE_VCPUS       (default ${CONTROL_PLANE_VCPUS})
  CONTROL_PLANE_MEMORY_MIB  (default ${CONTROL_PLANE_MEMORY_MIB})
  WORKER_VCPUS              (default ${WORKER_VCPUS})
  WORKER_MEMORY_MIB         (default ${WORKER_MEMORY_MIB})
  QARAX_TOKEN               API token (default: the local stack's)
EOF
}

# ── Helpers ──────────────────────────────────────────────────────────────────

# node_put PATH: write stdin to PATH inside the node container.
node_put() {
	docker exec -i "$NODE_CID" sh -c 'cat >"$1"' sh "$1"
}

# render FILE NAME=VALUE...: FILE with every @NAME@ replaced by VALUE.
render() {
	local file="$1" pair args=()
	shift
	for pair in "$@"; do
		args+=(-e "s|@${pair%%=*}@|${pair#*=}|g")
	done
	sed "${args[@]}" "$file"
}

# The registered host for the node container at address $NODE_SERVICE.
node_host() {
	qarax host list -o json |
		jq -r --arg a "$NODE_SERVICE" --arg s "$1" '.[] | select(.address == $a and ($s == "" or .status == $s)) | .name'
}

# The node-side file behind a disk object.
disk_path() {
	qarax storage-object get "$1" -o json | jq -er '.config.path'
}

vm_ssh() {
	local vm="$1"
	shift
	ssh "${SSH_OPTS[@]}" -p "${SSH_PORTS[$vm]}" "${DEBUG_USER}@${SSH_HOSTS[$vm]}" "$@"
}

# start_relay PORT TARGET_IP TARGET_PORT: forward PORT on the node container
# to the VM. The outgoing side binds to the gateway address so the VM's
# replies return over the bridge.
start_relay() {
	stop_relay "$1"
	docker exec -d "$NODE_CID" \
		socat "TCP-LISTEN:$1,bind=0.0.0.0,reuseaddr,fork" "TCP:$2:$3,bind=${GATEWAY}"
}

# Kill the socat listening on PORT. Scans /proc because the node image has no
# fuser; a process can exit mid-scan, hence the silenced cmdline reads.
stop_relay() {
	node_exec "$NODE_SERVICE" sh -c '
		for p in /proc/[0-9]*; do
			tr "\0" " " <"$p/cmdline" 2>/dev/null | grep -q "^socat TCP-LISTEN:$1," &&
				kill "${p#/proc/}"
		done
		true
	' sh "$1"
}

stop_relays() {
	local port
	for port in "$API_RELAY_PORT" "$SMOKE_RELAY_PORT" "${SSH_RELAY_PORTS[@]}"; do
		stop_relay "$port"
	done
}

setup_direct_access() {
	if ip link show "$VETH_HOST" >/dev/null 2>&1; then
		ok "Host veth ${VETH_HOST} is plumbed into ${BRIDGE_NAME}"
		return
	fi
	local pid
	pid=$(docker inspect -f '{{.State.Pid}}' "$NODE_CID")
	sudo ip link add "$VETH_HOST" type veth peer name "$VETH_PEER"
	sudo ip link set "$VETH_PEER" netns "$pid"
	sudo nsenter -t "$pid" -n ip link set "$VETH_PEER" master "$BRIDGE_NAME" up
	sudo ip addr add "$HOST_ACCESS_IP" dev "$VETH_HOST"
	sudo ip link set "$VETH_HOST" up
	ok "Host veth ${VETH_HOST} → ${BRIDGE_NAME} in ${NODE_SERVICE}"
}

teardown_direct_access() {
	if ip link show "$VETH_HOST" >/dev/null 2>&1; then
		sudo ip link del "$VETH_HOST"
		ok "Removed host veth ${VETH_HOST}"
	fi
}

# ── Steps ────────────────────────────────────────────────────────────────────

preflight() {
	step "Preflight"
	[[ "$(id -u)" != 0 ]] || die "Run as a regular user: the demo calls sudo itself, only for the optional host veth."
	local cmd
	for cmd in docker jq kubectl ssh ssh-keygen curl; do
		command -v "$cmd" >/dev/null || die "${cmd} is required"
	done
	[[ -e /dev/kvm ]] || die "/dev/kvm not found: KVM is required"
	mkdir -p "$DEMO_HOME"
	chmod 700 "$DEMO_HOME"
	ok "Tools and /dev/kvm available; state in ${DEMO_HOME}"
}

select_node() {
	step "Second node (${NODE_SERVICE})"
	ensure_up_hosts 2
	HOST_NAME=$(node_host up)
	[[ -n "$HOST_NAME" ]] || die "No UP host has address ${NODE_SERVICE}: this demo runs its VMs on the local stack's ${NODE_SERVICE} container."
	NODE_CID=$(compose ps -q "$NODE_SERVICE")
	[[ -n "$NODE_CID" ]] || die "Host ${HOST_NAME} is UP, but the compose service ${NODE_SERVICE} isn't running here."
	ok "VMs go to host ${HOST_NAME}"
}

ensure_network() {
	step "Network ${NETWORK_NAME} (${SUBNET}) on ${HOST_NAME}"
	if exists network "$NETWORK_NAME"; then
		ok "Network exists"
	else
		run qarax network create --name "$NETWORK_NAME" --subnet "$SUBNET" --gateway "$GATEWAY"
	fi
	# Pin it to HOST_NAME: the cluster's nodes talk over the bridge.
	pin_network "$NETWORK_NAME" "$HOST_NAME" "$BRIDGE_NAME"
}

ensure_pool() {
	step "Local storage pool ${POOL_NAME}"
	if exists storage-pool "$POOL_NAME"; then
		ok "Pool exists"
	else
		run qarax storage-pool create --name "$POOL_NAME" --pool-type local --path "$POOL_PATH" --host "$HOST_NAME"
	fi
}

bake_base_image() {
	local fedora="${NODE_CACHE_DIR}/${FEDORA_CLOUD_URL##*/}"
	if ! node_exec "$NODE_SERVICE" test -f "$fedora"; then
		info "Downloading ${FEDORA_CLOUD_URL##*/} into ${NODE_SERVICE}..."
		node_exec "$NODE_SERVICE" curl -fL --retry 5 --retry-delay 5 -C - -o "${fedora}.part" "$FEDORA_CLOUD_URL"
		node_exec "$NODE_SERVICE" mv "${fedora}.part" "$fedora"
	fi
	info "Installing Kubernetes ${KUBERNETES_MINOR} and its images with virt-customize (~10-15 min)..."
	render "${DEMO_DIR}/prebake.sh" KUBERNETES_MINOR="$KUBERNETES_MINOR" FLANNEL_VERSION="$FLANNEL_VERSION" |
		node_put /tmp/k8s-prebake.sh
	# Older bakes (other versions, interrupted runs) are dead weight.
	node_exec "$NODE_SERVICE" sh -c 'rm -f "$1"/base-k8s-*' sh "$NODE_CACHE_DIR"
	node_exec "$NODE_SERVICE" cp "$fedora" "${BASE_IMAGE}.tmp"
	node_exec "$NODE_SERVICE" env LIBGUESTFS_BACKEND=direct \
		virt-customize -a "${BASE_IMAGE}.tmp" --run /tmp/k8s-prebake.sh --selinux-relabel
	node_exec "$NODE_SERVICE" mv "${BASE_IMAGE}.tmp" "$BASE_IMAGE"
	node_exec "$NODE_SERVICE" rm /tmp/k8s-prebake.sh
}

ensure_base_image() {
	step "Base image with Kubernetes ${KUBERNETES_MINOR} baked in"
	if node_exec "$NODE_SERVICE" test -f "$BASE_IMAGE"; then
		ok "Cached in ${NODE_SERVICE}: ${BASE_IMAGE}"
		return
	fi
	node_exec "$NODE_SERVICE" mkdir -p "$NODE_CACHE_DIR"
	if [[ -f "$BASE_CACHE" ]]; then
		info "Copying ${BASE_CACHE} into ${NODE_SERVICE}..."
		docker cp "$BASE_CACHE" "${NODE_CID}:${BASE_IMAGE}.tmp"
		node_exec "$NODE_SERVICE" mv "${BASE_IMAGE}.tmp" "$BASE_IMAGE"
	else
		bake_base_image
		info "Keeping a copy in ${BASE_CACHE}..."
		rm -f "${DEMO_HOME}"/base-k8s-*
		docker cp "${NODE_CID}:${BASE_IMAGE}" "${BASE_CACHE}.tmp"
		mv "${BASE_CACHE}.tmp" "$BASE_CACHE"
	fi
	ok "Base image ready: ${BASE_IMAGE}"
}

# Each VM gets a blank raw disk from qarax, and the base image is written
# straight into it: one copy per VM, all three at once.
create_vm_disks() {
	step "VM disks (${DISK_SIZE} each)"
	local vm path pids=() pid
	for vm in "${VMS[@]}"; do
		run qarax storage-pool create-disk --pool "$POOL_NAME" --name "${vm}-disk" --size "$DISK_SIZE"
	done
	info "Writing the base image into the three disks..."
	for vm in "${VMS[@]}"; do
		path=$(disk_path "${vm}-disk")
		node_exec "$NODE_SERVICE" qemu-img convert -n --target-is-zero -f qcow2 -O raw "$BASE_IMAGE" "$path" &
		pids+=("$!")
	done
	for pid in "${pids[@]}"; do
		wait "$pid" || die "qemu-img convert failed (see above)"
	done
	ok "Disks ready"
}

prepare_access() {
	step "Host access to ${SUBNET}"
	local vm
	# `sudo -n` fails (asking for a password) when sudo isn't passwordless.
	if sudo -n true 2>/dev/null; then
		ACCESS_MODE=direct
		setup_direct_access
		API_URL="https://${CONTROL_IP}:6443"
		SMOKE_URL="http://${CONTROL_IP}:${SMOKE_NODEPORT}/"
		for vm in "${VMS[@]}"; do
			SSH_HOSTS["$vm"]="${VM_IPS[$vm]}"
			SSH_PORTS["$vm"]=22
		done
		ok "VMs are reachable at their own addresses"
	else
		ACCESS_MODE=relay
		local container_ip
		container_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$NODE_CID")
		start_relay "$API_RELAY_PORT" "$CONTROL_IP" 6443
		start_relay "$SMOKE_RELAY_PORT" "$CONTROL_IP" "$SMOKE_NODEPORT"
		for vm in "${VMS[@]}"; do
			start_relay "${SSH_RELAY_PORTS[$vm]}" "${VM_IPS[$vm]}" 22
			SSH_HOSTS["$vm"]="$container_ip"
			SSH_PORTS["$vm"]="${SSH_RELAY_PORTS[$vm]}"
		done
		API_URL="https://${container_ip}:${API_RELAY_PORT}"
		SMOKE_URL="http://${container_ip}:${SMOKE_RELAY_PORT}/"
		ok "No passwordless sudo: socat relays on ${container_ip} (API port ${API_RELAY_PORT})"
	fi
}

# render_user_data VM ROLE TOKEN: cloud-config that adds the debug user and
# runs node-setup.sh once.
render_user_data() {
	local vm="$1" role="$2" token="$3" script
	script=$(render "${DEMO_DIR}/node-setup.sh" \
		ROLE="$role" NODE_IP="${VM_IPS[$vm]}" CONTROL_IP="$CONTROL_IP" GATEWAY="$GATEWAY" \
		TOKEN="$token" DEBUG_USER="$DEBUG_USER" | base64 -w 0)
	cat >"${DEMO_HOME}/user-data-${vm}.yaml" <<EOF
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
      - '$(<"${SSH_KEY}.pub")'
write_files:
  - path: /usr/local/bin/k8s-setup.sh
    permissions: '0755'
    encoding: b64
    content: ${script}
runcmd:
  - /usr/local/bin/k8s-setup.sh
EOF
}

create_vms() {
	step "Creating the VMs (firmware boot, cloud-init)"
	[[ -f "$SSH_KEY" ]] || ssh-keygen -q -t ed25519 -N "" -C "qarax-k8s-demo" -f "$SSH_KEY"
	# A kubeadm bootstrap token ([a-z0-9]{6}.[a-z0-9]{16}), shared by all nodes.
	local token vm role vcpus memory
	token="$(od -An -N3 -tx1 /dev/urandom | tr -d ' \n').$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')"
	for vm in "${VMS[@]}"; do
		if [[ "$vm" == "$CONTROL_VM" ]]; then
			role=control vcpus="$CONTROL_PLANE_VCPUS" memory="$CONTROL_PLANE_MEMORY_MIB"
		else
			role=worker vcpus="$WORKER_VCPUS" memory="$WORKER_MEMORY_MIB"
		fi
		render_user_data "$vm" "$role" "$token"
		run qarax vm create \
			--name "$vm" \
			--boot-mode firmware \
			--vcpus "$vcpus" \
			--memory "${memory}MiB" \
			--network "$NETWORK_NAME" \
			--ip "${VM_IPS[$vm]}" \
			--offload-tso false \
			--offload-ufo false \
			--offload-csum false \
			--root-disk "${vm}-disk" \
			--cloud-init-user-data "${DEMO_HOME}/user-data-${vm}.yaml"
	done
}

# node-setup.sh's status ("running", "done" or "failed"), or nothing while
# the VM is booting: SSH only answers once cloud-init has configured the
# network and the debug user, so failing here is expected for a while.
setup_status() {
	vm_ssh "$1" cat /var/lib/k8s-setup/status 2>/dev/null || true
}

wait_for_setup() {
	step "Waiting for kubeadm init / join on all three VMs"
	info "Polled over SSH every 5 s; guest log: /var/log/k8s-setup.log"
	local deadline=$((SECONDS + SETUP_TIMEOUT)) vm status pending=("${VMS[@]}") left
	local -A seen=()
	while true; do
		left=()
		for vm in "${pending[@]}"; do
			status=$(vm_status "$vm")
			[[ "$status" == "running" ]] || die "VM ${vm} is '${status}', expected running"
			status=$(setup_status "$vm")
			if [[ "$status" != "${seen[$vm]:-}" ]]; then
				info "${vm}: ${status:-booting}"
				seen["$vm"]="$status"
			fi
			case "$status" in
			"done") ;;
			failed)
				vm_ssh "$vm" sudo tail -n 30 /var/log/k8s-setup.log >&2 || warn "Could not read ${vm}'s setup log"
				die "Setup failed on ${vm}; more in /var/log/k8s-setup-debug.log on the VM"
				;;
			*) left+=("$vm") ;;
			esac
		done
		pending=("${left[@]}")
		((${#pending[@]} > 0)) || break
		((SECONDS < deadline)) || die "Timed out after ${SETUP_TIMEOUT}s waiting for: ${pending[*]}"
		sleep 5
	done
	ok "Control plane initialised and both workers joined"
}

fetch_kubeconfig() {
	step "Fetching the admin kubeconfig over SSH"
	vm_ssh "$CONTROL_VM" sudo cat /etc/kubernetes/admin.conf >"$KUBECONFIG_PATH" ||
		die "Could not read /etc/kubernetes/admin.conf on ${CONTROL_VM}"
	export KUBECONFIG="$KUBECONFIG_PATH"
	if [[ "$ACCESS_MODE" == relay ]]; then
		# The relay address isn't in the API server certificate; keep
		# verifying it against the control plane's own IP.
		kubectl config set-cluster "$(kubectl config view -o jsonpath='{.clusters[0].name}')" \
			--server="$API_URL" --tls-server-name="$CONTROL_IP" >/dev/null
	fi
	ok "Saved to ${KUBECONFIG_PATH} (API server ${API_URL})"
}

all_nodes_registered() {
	[[ "$(kubectl get nodes -o name | wc -l)" -eq "${#VMS[@]}" ]]
}

verify_cluster() {
	step "Cluster nodes"
	wait_until 60 all_nodes_registered ||
		die "Expected ${#VMS[@]} nodes; kubectl sees: $(kubectl get nodes -o name | tr '\n' ' ')"
	run kubectl wait --for=condition=Ready node --all --timeout=300s
	run kubectl get nodes -o wide

	step "Smoke test: a Deployment behind a NodePort Service"
	run kubectl apply -f "${DEMO_DIR}/smoke.yaml"
	run kubectl -n demo rollout status deployment/hello-k8s --timeout=300s
	run curl -sf --max-time 5 --retry 6 --retry-all-errors --retry-delay 5 "$SMOKE_URL" ||
		die "The smoke workload did not answer at ${SMOKE_URL}"
	ok "The smoke workload answered through NodePort ${SMOKE_NODEPORT}"
}

print_summary() {
	banner "3-node Kubernetes ${KUBERNETES_MINOR} cluster is running"
	echo "  export KUBECONFIG=${KUBECONFIG_PATH}"
	echo "  kubectl get nodes"
	echo
	echo "  API server: ${API_URL}"
	local vm
	for vm in "${VMS[@]}"; do
		echo "  ssh ${vm}: ssh -i ${SSH_KEY} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -p ${SSH_PORTS[$vm]} ${DEBUG_USER}@${SSH_HOSTS[$vm]}"
	done
	echo
	echo -e "  Tear down: ${CYAN}$0 --cleanup${NC} (--purge also drops the cached base image)"
}

cleanup() {
	step "Removing the demo's VMs, disks, network and storage pool"
	NODE_CID=$(compose ps -q "$NODE_SERVICE")
	sweep vm "$PREFIX"
	sweep storage-object "$PREFIX"
	if exists network "$NETWORK_NAME"; then
		# Also detaches it from the node: bridge, DHCP server and NAT rules.
		run qarax network delete "$NETWORK_NAME"
	fi
	sweep storage-pool "$PREFIX"
	# Relays live in the node container and go away with it.
	[[ -z "$NODE_CID" ]] || stop_relays
	teardown_direct_access
	rm -f "$KUBECONFIG_PATH" "${DEMO_HOME}"/user-data-*.yaml
	ok "Done; the cached base image is kept (--purge removes it)"
}

purge() {
	cleanup
	step "Removing the cached base image and SSH key"
	[[ -z "$NODE_CID" ]] || node_exec "$NODE_SERVICE" rm -rf "$NODE_CACHE_DIR"
	rm -rf "$DEMO_HOME"
	ok "Removed ${DEMO_HOME} and ${NODE_SERVICE}:${NODE_CACHE_DIR}"
}

# ── Main ─────────────────────────────────────────────────────────────────────

case "${1:-}" in
-h | --help)
	usage
	exit 0
	;;
--cleanup | --purge)
	require_server
	require_cli
	if [[ "$1" == --cleanup ]]; then cleanup; else purge; fi
	exit 0
	;;
"") ;;
*)
	usage >&2
	exit 1
	;;
esac

banner "Kubernetes ${KUBERNETES_MINOR} cluster on qarax (kubeadm, 3 VMs)"
preflight
ensure_stack
require_cli
select_node

step "Clearing the previous run's VMs and disks"
sweep vm "$PREFIX"
sweep storage-object "$PREFIX"
ok "Nothing left from a previous run"

ensure_network
ensure_pool
ensure_base_image
create_vm_disks
prepare_access
create_vms

step "Starting the VMs"
start_vms "${VMS[@]}"

wait_for_setup
fetch_kubeconfig
verify_cluster
print_summary
