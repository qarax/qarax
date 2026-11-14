#!/usr/bin/env bash
#
# Demo: Boot a VM from a cloud image with cloud-init credential injection
#
# The cloud image workflow:
#   1. Create a local storage pool and a bridge network
#   2. Download a cloud image (Ubuntu 22.04 minimal by default) into the pool,
#      once: later runs reuse it as a pristine base
#   3. Copy the base image to a root disk of the VM's own
#   4. Create a UEFI VM with that disk as root and cloud-init injecting your
#      SSH key and starting a small HTTP server
#   5. Start the VM and wait for the guest's HTTP server to answer
#
# The result is a VM with a standalone disk: no OCI registry, no OverlayBD,
# no external dependency after the download.
#
# The VM (demo-cloud-image-vm) is left running. A re-run replaces the VM and
# its disk, and reuses the pool, the network and the downloaded image.
#
# Prerequisites:
#   - qarax stack running (make run-local), or let the script start it
#   - cargo (to build the qarax CLI) or a qarax CLI on PATH
#   - jq, curl
#   - Internet access from the host for the image download
#   - An SSH public key at ~/.ssh/id_ed25519.pub or ~/.ssh/id_rsa.pub (or set SSH_PUB_KEY)
#
# Usage:
#   ./demos/cloud-image/run.sh
#   ./demos/cloud-image/run.sh --image-url https://example.com/custom.img --disk-name custom
#   ./demos/cloud-image/run.sh --cleanup   # delete the VM, its disk and the network
#   ./demos/cloud-image/run.sh --purge     # ...and the downloaded image and the pool
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

# Print the first SSH public key found in ~/.ssh (nothing if there is none).
default_ssh_key() {
	local f
	for f in ~/.ssh/id_ed25519.pub ~/.ssh/id_rsa.pub; do
		if [[ -r "$f" ]]; then
			cat "$f"
			return
		fi
	done
}

PREFIX="demo-cloud-image-"
VM_NAME="${PREFIX}vm"
VM_DISK_NAME="${VM_NAME}-root"
POOL_NAME="${PREFIX}pool"
POOL_PATH="/var/lib/qarax/storage/demo-cloud-image-pool" # on the node's volume, so the download survives a node container recreate
NETWORK_NAME="${PREFIX}net"
NETWORK_SUBNET="10.97.0.0/24"
NETWORK_GATEWAY="10.97.0.1"
BRIDGE_NAME="qci0"
PARENT_INTERFACE=""
BASE_DISK_NAME="ubuntu-22.04-cloud"
IMAGE_URL="https://cloud-images.ubuntu.com/minimal/releases/jammy/release/ubuntu-22.04-minimal-cloudimg-amd64.img"
HOST_NAME="${QARAX_HOST:-}"
VCPUS=2
MEMORY_GIB=1
SSH_PUB_KEY="${SSH_PUB_KEY:-$(default_ssh_key)}"
CLEANUP=0
PURGE=0
BOOT_TIMEOUT=180

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --image-url URL          Cloud image URL (default: Ubuntu 22.04 minimal)
  --disk-name NAME         Storage object name for the downloaded image (default: ubuntu-22.04-cloud)
  --pool-name NAME         Local storage pool name (default: demo-cloud-image-pool)
  --pool-path PATH         Pool directory on the host (default: /var/lib/qarax/cloud-image-pool)
  --network-name NAME      Network name (default: demo-cloud-image-net)
  --subnet CIDR            Network subnet (default: 10.97.0.0/24)
  --gateway IP             Network gateway (default: 10.97.0.1)
  --bridge-name NAME       Bridge device on the host (default: qci0)
  --parent-interface NIC   Bridge this host NIC instead of an isolated NAT bridge
  --host NAME              Host for the pool and network (default: \$QARAX_HOST or the first UP host)
  --vcpus N                Number of vCPUs (default: 2)
  --memory GiB             Memory in GiB (default: 1)
  --ssh-key KEY            SSH public key to inject (default: \$SSH_PUB_KEY or ~/.ssh/id_{ed25519,rsa}.pub)
  --server URL             qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)
  --cleanup                Delete the VM, its disk and the network (keep the downloaded image), then exit
  --purge                  Like --cleanup, and also delete the downloaded image and the pool
EOF
}

while [[ $# -gt 0 ]]; do
	case $1 in
	--image-url)
		IMAGE_URL="$2"
		shift 2
		;;
	--disk-name)
		BASE_DISK_NAME="$2"
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
	--network-name)
		NETWORK_NAME="$2"
		shift 2
		;;
	--subnet)
		NETWORK_SUBNET="$2"
		shift 2
		;;
	--gateway)
		NETWORK_GATEWAY="$2"
		shift 2
		;;
	--bridge-name)
		BRIDGE_NAME="$2"
		shift 2
		;;
	--parent-interface)
		PARENT_INTERFACE="$2"
		shift 2
		;;
	--host)
		HOST_NAME="$2"
		shift 2
		;;
	--vcpus)
		VCPUS="$2"
		shift 2
		;;
	--memory)
		MEMORY_GIB="$2"
		shift 2
		;;
	--ssh-key)
		SSH_PUB_KEY="$2"
		shift 2
		;;
	--server)
		SERVER="$2"
		shift 2
		;;
	--cleanup)
		CLEANUP=1
		shift
		;;
	--purge)
		CLEANUP=1
		PURGE=1
		shift
		;;
	--help | -h)
		usage
		exit 0
		;;
	*)
		usage >&2
		die "Unknown option: $1"
		;;
	esac
done

# pool_object_id POOL NAME: print the ID of storage object NAME in POOL (nothing if absent).
pool_object_id() {
	qarax storage-object list --pool "$1" -o json | jq -r --arg n "$2" '.[] | select(.name == $n) | .id'
}

# Delete the demo VM and its root disk copy. The downloaded base image stays.
remove_vm() {
	sweep vm "$PREFIX"
	sweep storage-object "$VM_DISK_NAME"
}

# guest_http_up SERVICE IP: whether the guest's HTTP server answers, probed
# from inside the node container SERVICE. Fails until cloud-init starts it.
guest_http_up() {
	node_exec "$1" curl -sf --max-time 2 -o /dev/null "http://$2:8080/"
}

remove_network() {
	if exists network "$NETWORK_NAME"; then
		run qarax network delete "$NETWORK_NAME"
	fi
}

if ((CLEANUP)); then
	if ! server_reachable; then
		info "qarax server not reachable at ${SERVER}: nothing to clean up."
		exit 0
	fi
	require_auth
	require_cli
	[[ -n "$HOST_NAME" ]] || HOST_NAME="$(first_up_host)"
	step "Deleting cloud-image demo resources"
	remove_vm
	remove_network
	if ((PURGE)); then
		if exists storage-object "$BASE_DISK_NAME"; then
			run qarax storage-object delete "$BASE_DISK_NAME"
		fi
		if exists storage-pool "$POOL_NAME"; then
			run qarax storage-pool delete "$POOL_NAME"
		fi
	else
		info "Kept the downloaded image ${BASE_DISK_NAME} and pool ${POOL_NAME} (--purge deletes them)"
	fi
	ok "Done"
	exit 0
fi

[[ -n "$SSH_PUB_KEY" ]] ||
	die "No SSH public key found. Set SSH_PUB_KEY, pass --ssh-key, or create ~/.ssh/id_ed25519.pub."

ensure_stack
require_cli
[[ -n "$HOST_NAME" ]] || HOST_NAME="$(first_up_host)"

banner "Cloud Image VM Demo"
info "Image URL:  ${IMAGE_URL}"
info "VM:         ${VM_NAME} (${VCPUS} vCPU, ${MEMORY_GIB} GiB)"
info "Host:       ${HOST_NAME}"
info "Pool:       ${POOL_NAME} (${POOL_PATH})"
info "Network:    ${NETWORK_NAME} (${NETWORK_SUBNET})"

# The VM and its disk belong to this demo: replace them on every run.
remove_vm

step "1. Storage pool"
if exists storage-pool "$POOL_NAME"; then
	ok "${POOL_NAME} exists, reusing it"
else
	run qarax storage-pool create --name "$POOL_NAME" --pool-type local \
		--path "$POOL_PATH" --host "$HOST_NAME"
fi

step "2. Network"
if exists network "$NETWORK_NAME"; then
	ok "${NETWORK_NAME} exists, reusing it"
	info "Network flags apply when it is created; run --cleanup first to change them"
else
	run qarax network create --name "$NETWORK_NAME" --subnet "$NETWORK_SUBNET" --gateway "$NETWORK_GATEWAY"
fi
# On every UP host (attaching is safe to repeat), so the scheduler can place
# the VM wherever the pool holding its disk lives.
for host in $(qarax host list -o json | jq -r '.[] | select(.status == "up") | .name'); do
	ATTACH_ARGS=(--network "$NETWORK_NAME" --host "$host" --bridge-name "$BRIDGE_NAME")
	if [[ -n "$PARENT_INTERFACE" ]]; then
		ATTACH_ARGS+=(--parent-interface "$PARENT_INTERFACE")
	fi
	run qarax network attach-host "${ATTACH_ARGS[@]}"
done

step "3. Cloud image"
BASE_ID="$(pool_object_id "$POOL_NAME" "$BASE_DISK_NAME")"
if [[ -n "$BASE_ID" ]]; then
	ok "${BASE_DISK_NAME} was downloaded by an earlier run, reusing it (--purge deletes it)"
else
	info "Downloading into the pool; this can take a few minutes"
	run qarax storage-pool create-disk --pool "$POOL_NAME" --name "$BASE_DISK_NAME" --source "$IMAGE_URL"
	BASE_ID="$(pool_object_id "$POOL_NAME" "$BASE_DISK_NAME")"
	[[ -n "$BASE_ID" ]] || die "Download finished but ${BASE_DISK_NAME} is not in ${POOL_NAME}"
fi

step "4. Root disk for the VM: a copy of the downloaded image"
# The storage object's config holds its path on the host.
BASE_PATH="$(qarax storage-object get "$BASE_ID" -o json | jq -er '.config.path')"
run qarax transfer create --pool "$POOL_NAME" --name "$VM_DISK_NAME" \
	--source "$BASE_PATH" --object-type disk --wait

step "5. Create the VM with cloud-init user-data"
# A fixed address, configured statically through cloud-init network-config:
# the node's DHCP server doesn't reserve qarax's allocation for the VM's MAC,
# so a DHCP guest can come up on a different address than qarax recorded.
VM_IP="${NETWORK_SUBNET%.*}.10"
CLOUD_INIT_DIR="$(mktemp -d)"
trap 'rm -rf "$CLOUD_INIT_DIR"' EXIT
USER_DATA_FILE="${CLOUD_INIT_DIR}/user-data.yaml"
NETWORK_CONFIG_FILE="${CLOUD_INIT_DIR}/network-config.yaml"
cat >"$NETWORK_CONFIG_FILE" <<EOF
version: 2
ethernets:
  primary:
    match:
      name: "e*"
    addresses: [${VM_IP}/${NETWORK_SUBNET#*/}]
    routes:
      - to: default
        via: ${NETWORK_GATEWAY}
    nameservers:
      addresses: [1.1.1.1, 8.8.8.8]
EOF
cat >"$USER_DATA_FILE" <<EOF
#cloud-config
users:
  - name: qarax
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys:
      - ${SSH_PUB_KEY}
growpart:
  mode: auto
  devices: ['/']
resize_rootfs: true
runcmd:
  - [sh, -c, 'echo demo-cloud-image > /var/tmp/index.html']
  - [sh, -c, 'nohup python3 -m http.server 8080 --directory /var/tmp >/var/log/qarax-demo-http.log 2>&1 &']
EOF
run qarax vm create --name "$VM_NAME" --vcpus "$VCPUS" --memory "${MEMORY_GIB}GiB" \
	--boot-mode firmware --root-disk "$VM_DISK_NAME" --network "$NETWORK_NAME" --ip "$VM_IP" \
	--cloud-init-user-data "$USER_DATA_FILE" --cloud-init-network-config "$NETWORK_CONFIG_FILE"

step "6. Start the VM"
run qarax vm start "$VM_NAME"
run qarax vm get "$VM_NAME"

VM_JSON="$(qarax vm get "$VM_NAME" -o json)"
HOST_ADDRESS="$(qarax host get "$(jq -r '.host_id' <<<"$VM_JSON")" -o json | jq -r '.address')"

step "7. Wait for cloud-init"
# The bridge lives on the host, so the guest is probed from the node container.
if NODE_SERVICE="$(node_service_for_address "$HOST_ADDRESS")"; then
	info "cloud-init creates the qarax user and starts an HTTP server on ${VM_IP}:8080 (up to ${BOOT_TIMEOUT}s)"
	wait_until "$BOOT_TIMEOUT" guest_http_up "$NODE_SERVICE" "$VM_IP" ||
		die "The guest's HTTP server did not answer within ${BOOT_TIMEOUT}s.\nWatch the boot with: qarax vm attach ${VM_NAME}"
	BODY="$(node_exec "$NODE_SERVICE" curl -sf --max-time 5 "http://${VM_IP}:8080/")"
	[[ "$BODY" == "demo-cloud-image" ]] || die "Unexpected response from the guest: ${BODY}"
	ok "The guest at ${VM_IP} answers '${BODY}' on port 8080: cloud-init ran"
else
	info "The VM's host (${HOST_ADDRESS}) is not a local compose node, so the demo can't reach"
	info "the guest. From a machine on its network: curl http://${VM_IP}:8080/"
fi

banner "Done"
echo "VM ${VM_NAME} is running at ${VM_IP}. cloud-init created user 'qarax' with your SSH key."
if [[ -n "$PARENT_INTERFACE" ]]; then
	echo "The network is bridged to '${PARENT_INTERFACE}', so the VM is on that network:"
	echo "  ssh qarax@${VM_IP}"
else
	echo "The bridge is isolated on the host; reach the guest from the node, e.g.:"
	if [[ "$HOST_ADDRESS" == bootc-vm ]]; then
		# The node is the bootc microVM (make run-local VM=1); its shell is behind the runner container.
		echo "  docker compose -f ${REPO_ROOT}/e2e/docker-compose.yml exec bootc-vm ssh root@10.0.2.15 curl http://${VM_IP}:8080/"
	else
		echo "  docker compose -f ${REPO_ROOT}/e2e/docker-compose.yml exec ${NODE_SERVICE:-qarax-node} curl http://${VM_IP}:8080/"
	fi
fi
echo ""
echo "Useful commands:"
echo "  qarax vm get ${VM_NAME}"
echo "  qarax vm attach ${VM_NAME}   # interactive serial console (Ctrl+] detaches)"
echo "  $0 --cleanup   # delete the VM, its disk and the network (keeps the downloaded image)"
echo "  $0 --purge     # also delete the downloaded image and the pool"
