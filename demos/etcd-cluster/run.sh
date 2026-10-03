#!/usr/bin/env bash
#
# Demo: self-contained 3-node etcd cluster on qarax
#
# Starts the qarax stack if needed (hack/run-local.sh), builds the etcd node
# image, converts it to OverlayBD, imports it, and boots three VMs on an
# isolated network.
#
# Network layout:
#   etcd-net  10.100.0.0/24  (isolated bridge + NAT)
#   etcd-0    10.100.0.10
#   etcd-1    10.100.0.11
#   etcd-2    10.100.0.12
#
# Requirements:
#   - Docker (with Compose)
#   - podman  (to build the etcd node OCI image)
#   - jq
#   - KVM     (/dev/kvm)
#   - Rust toolchain
#   - sudo    (to plumb a host veth into the VM bridge)
#
# Environment:
#   QARAX_TOKEN  API token (default: the local stack's e2e-test-token)
#   QARAX_HOST   host to attach etcd-net to (default: the host at address qarax-node)
#
# Usage:
#   ./demos/etcd-cluster/run.sh           # run everything
#   ./demos/etcd-cluster/run.sh --cleanup # remove host veth and tear down the stack

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEMO_DIR="$(cd "$(dirname "$0")" && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

cd "$REPO_ROOT"

# Configuration

NETWORK_NAME="etcd-net"
SUBNET="10.100.0.0/24"
GATEWAY="10.100.0.1"
BRIDGE_NAME="br-etcd"
VETH_HOST="veth-host"
VETH_VM="veth-vm"
HOST_ACCESS_IP="10.100.0.254/24"
POOL_NAME="overlaybd-pool"
REGISTRY_URL="http://registry:5000"
IMAGE_REF="registry:5000/etcd-node:latest"
IMAGE_OBJECT_NAME="etcd-node"
NODE_ADDRESS="qarax-node"
NODE_PORT=50051
SERVER="http://localhost:8000"
VCPUS=1
MEMORY_MIB=256

NODES=(etcd-0 etcd-1 etcd-2)
declare -A NODE_IPS=([etcd - 0]="10.100.0.10" [etcd - 1]="10.100.0.11" [etcd - 2]="10.100.0.12")

MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))
COMPOSE=(docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml")
NODE_SERVICE="qarax-node"

step() { echo -e "\n${CYAN}=== $* ===${NC}"; }
ok() { echo -e "${GREEN}✓ $*${NC}"; }
info() { echo -e "${YELLOW}$*${NC}"; }

# Run a command inside the qarax-node container (where the VM bridge lives).
node_exec() { "${COMPOSE[@]}" exec -T "$NODE_SERVICE" "$@"; }

vm_status() { "${QARAX[@]}" vm get "$1" -o json | jq -r '.status'; }

# Locate qarax CLI

QARAX_BIN="$(find_qarax_bin)"
if [[ -z "$QARAX_BIN" ]]; then
	echo "qarax CLI not found — building..."
	cargo build -p cli
	QARAX_BIN="$(find_qarax_bin)"
fi
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"
QARAX=("$QARAX_BIN" --server "$SERVER")

# Cleanup

cleanup() {
	step "Cleaning up etcd cluster demo"

	if ip link show "$VETH_HOST" &>/dev/null; then
		sudo ip link del "$VETH_HOST" 2>/dev/null || true
		ok "Host veth removed"
	fi

	# Tearing down the stack (containers + volumes) removes the VMs, the
	# network, and the imported image along with it.
	bash "${REPO_ROOT}/hack/run-local.sh" --cleanup
	ok "Stack torn down"
}

if [[ "${1:-}" == "--cleanup" ]]; then
	cleanup
	exit 0
fi

# Preflight

step "Preflight checks"

command -v docker &>/dev/null || die "docker is required"
command -v podman &>/dev/null || die "podman is required (to build the etcd image)"
command -v jq &>/dev/null || die "jq is required (to parse JSON output)"
[[ -e /dev/kvm ]] || die "/dev/kvm not found — KVM is required"

if [[ ! -e /dev/net/tun ]]; then
	info "Warning: /dev/net/tun not found — VM networking will fail."
	info "Fix: sudo modprobe tun && sudo mkdir -p /dev/net && sudo mknod /dev/net/tun c 10 200 && sudo chmod 0666 /dev/net/tun"
fi

ok "Preflight passed"

# Start the qarax stack (hack/run-local.sh) if it is not already running

step "Ensuring qarax stack is running"
ensure_stack "$SERVER"
ok "qarax API is ready at $SERVER"

# Host + overlaybd storage pool (run-local.sh normally creates both)

step "Checking host and overlaybd storage pool"

HOST_NAME="${QARAX_HOST:-$("${QARAX[@]}" host list -o json |
	jq -r --arg addr "$NODE_ADDRESS" '[.[] | select(.address == $addr)][0].name // empty')}"
if [[ -z "$HOST_NAME" ]]; then
	HOST_NAME="local-node"
	"${QARAX[@]}" host add --name "$HOST_NAME" --address "$NODE_ADDRESS" --port "$NODE_PORT" --user root
fi

if [[ "$("${QARAX[@]}" host get "$HOST_NAME" -o json | jq -r '.status')" == "up" ]]; then
	ok "Host '$HOST_NAME' is up"
else
	# Init with retries (qarax-node may still be starting)
	for attempt in 1 2 3 4 5; do
		if "${QARAX[@]}" host init "$HOST_NAME" >/dev/null 2>&1; then
			ok "Host '$HOST_NAME' initialized"
			break
		fi
		[[ $attempt -lt 5 ]] || die "Could not initialize host '$HOST_NAME' after 5 attempts"
		info "Host init attempt $attempt/5 failed, retrying..."
		sleep 3
	done
fi

if "${QARAX[@]}" storage-pool get "$POOL_NAME" &>/dev/null; then
	ok "Storage pool '$POOL_NAME' already exists"
else
	"${QARAX[@]}" storage-pool create --name "$POOL_NAME" --pool-type overlaybd --url "$REGISTRY_URL"
	# The server attaches UP hosts in the background; do it explicitly so the
	# import below does not race the attachment.
	"${QARAX[@]}" storage-pool attach-host "$POOL_NAME" --all >/dev/null 2>&1 || true
	ok "Storage pool '$POOL_NAME' created"
fi

# Build and push etcd node image

step "Building etcd node OCI image"

podman build -f "${DEMO_DIR}/Containerfile" -t localhost:5001/etcd-node:src "${DEMO_DIR}/"
ok "Image built"

step "Pushing etcd image to local registry"
# Push to a staging tag to avoid overwriting the overlaybd-converted :latest
podman push --tls-verify=false localhost:5001/etcd-node:src
ok "Image pushed as registry:5000/etcd-node:src"

step "Converting to OverlayBD format"
# The convertor converts :src → :latest in overlaybd block format.
# Running on every demo invocation is fast when nothing changed (registry
# returns the existing manifest) and keeps :latest in sync with :src.
node_exec /opt/overlaybd/snapshotter/convertor \
	--repository "registry:5000/etcd-node" \
	--input-tag src \
	--overlaybd latest \
	--plain
ok "OverlayBD conversion complete ($IMAGE_REF)"

# Demo

echo ""
echo "╔══════════════════════════════════════════════╗"
echo "║   qarax demo — 3-node etcd cluster           ║"
echo "╚══════════════════════════════════════════════╝"
echo ""
echo "  Image:   $IMAGE_REF"
echo "  Network: $NETWORK_NAME ($SUBNET)"
echo "  Nodes:   ${NODES[*]}"
echo ""

step "Step 1: Create isolated network"
if "${QARAX[@]}" network get "$NETWORK_NAME" &>/dev/null; then
	ok "Network '$NETWORK_NAME' already exists"
	# Re-attach in case a previous run failed between create and attach.
	"${QARAX[@]}" network attach-host --network "$NETWORK_NAME" --host "$HOST_NAME" \
		--bridge-name "$BRIDGE_NAME" >/dev/null 2>&1 || true
else
	"${QARAX[@]}" network create --name "$NETWORK_NAME" --subnet "$SUBNET" --gateway "$GATEWAY"
	"${QARAX[@]}" network attach-host --network "$NETWORK_NAME" --host "$HOST_NAME" --bridge-name "$BRIDGE_NAME"
	ok "Network ready"
fi

step "Step 2: Import etcd image into storage pool"
if "${QARAX[@]}" storage-object get "$IMAGE_OBJECT_NAME" &>/dev/null; then
	ok "Storage object '$IMAGE_OBJECT_NAME' already exists"
else
	"${QARAX[@]}" storage-pool import --pool "$POOL_NAME" --image-ref "$IMAGE_REF" --name "$IMAGE_OBJECT_NAME"
	ok "Image imported as '$IMAGE_OBJECT_NAME'"
fi

step "Step 3: Create VMs with static IPs"
for node in "${NODES[@]}"; do
	ip="${NODE_IPS[$node]}"
	if "${QARAX[@]}" vm get "$node" &>/dev/null; then
		ok "VM $node already exists"
	else
		"${QARAX[@]}" vm create --name "$node" --vcpus "$VCPUS" --memory "$MEMORY_BYTES" \
			--network "$NETWORK_NAME" --ip "$ip"
		ok "VM $node ($ip)"
	fi
done

step "Step 4: Attach etcd disk to each VM"
for node in "${NODES[@]}"; do
	status=$(vm_status "$node")
	if [[ "$status" != "created" ]]; then
		ok "Skipping disk attach for $node (status: $status)"
	else
		"${QARAX[@]}" vm attach-disk "$node" --object "$IMAGE_OBJECT_NAME"
		ok "Disk attached to $node"
	fi
done

step "Step 5: Start the cluster"
for node in "${NODES[@]}"; do
	status=$(vm_status "$node")
	if [[ "$status" == "running" ]]; then
		ok "$node already running"
	else
		"${QARAX[@]}" vm start "$node"
		ok "$node started"
	fi
done

step "Step 6: Wait for etcd cluster to be ready"

ETCD_ENDPOINTS="http://10.100.0.10:2379,http://10.100.0.11:2379,http://10.100.0.12:2379"
BOOT_TIMEOUT=120
elapsed=0

info "Waiting for all 3 etcd nodes to be reachable..."
while [[ $elapsed -lt $BOOT_TIMEOUT ]]; do
	all_up=true
	for node in "${NODES[@]}"; do
		ip="${NODE_IPS[$node]}"
		if ! node_exec nc -z -w2 "$ip" 2379 &>/dev/null; then
			all_up=false
			break
		fi
	done

	if $all_up; then
		ok "All 3 etcd nodes are reachable"
		break
	fi

	echo -n "."
	sleep 3
	elapsed=$((elapsed + 3))
done
echo ""
[[ $elapsed -lt $BOOT_TIMEOUT ]] ||
	die "Timeout after ${BOOT_TIMEOUT}s waiting for etcd nodes to boot.\nInspect a boot log with: $QARAX_BIN --server $SERVER vm console etcd-0"

info "Checking etcd cluster health via HTTP..."
sleep 2
for node in "${NODES[@]}"; do
	ip="${NODE_IPS[$node]}"
	health=$(node_exec sh -c \
		"curl -sf http://${ip}:2379/health 2>/dev/null || echo '{\"health\":\"false\"}'")
	ok "$node ($ip): $health"
done

step "Step 7: Make VM network accessible from the host"

if ip link show "$VETH_HOST" &>/dev/null; then
	ok "Host veth already exists"
else
	NODE_CONTAINER=$("${COMPOSE[@]}" ps -q "$NODE_SERVICE")
	[[ -n "$NODE_CONTAINER" ]] || die "Could not find the $NODE_SERVICE container"
	NODE_PID=$(docker inspect -f '{{.State.Pid}}' "$NODE_CONTAINER")
	sudo ip link add "$VETH_HOST" type veth peer name "$VETH_VM"
	sudo ip link set "$VETH_VM" netns "$NODE_PID"
	sudo nsenter -t "$NODE_PID" -n ip link set "$VETH_VM" master "$BRIDGE_NAME"
	sudo nsenter -t "$NODE_PID" -n ip link set "$VETH_VM" up
	sudo ip link set "$VETH_HOST" up
	sudo ip addr add "$HOST_ACCESS_IP" dev "$VETH_HOST"
	ok "Host veth created — VMs are directly reachable from the host"
fi

echo ""
echo "╔══════════════════════════════════════════════════════════════════╗"
echo "║  etcd cluster is READY                                           ║"
echo "╚══════════════════════════════════════════════════════════════════╝"
echo ""
for node in "${NODES[@]}"; do
	"${QARAX[@]}" vm get "$node"
	echo ""
done
echo "Cluster endpoints:"
for node in "${NODES[@]}"; do
	echo "  $node  http://${NODE_IPS[$node]}:2379"
done
echo ""
echo "Try it out (directly from your laptop):"
echo ""
echo "  # Cluster health:"
echo "  etcdctl endpoint health --endpoints=$ETCD_ENDPOINTS"
echo ""
echo "  # Write a key to one node, read from another:"
echo "  etcdctl --endpoints=http://10.100.0.10:2379 put hello world"
echo "  etcdctl --endpoints=http://10.100.0.11:2379 get hello"
echo ""
echo "  # The qarax CLI needs the API token:"
echo "  export QARAX_TOKEN=$QARAX_TOKEN"
echo ""
echo "  # Kill a node and show the cluster survives:"
echo "  $QARAX_BIN --server $SERVER vm stop etcd-2"
echo "  etcdctl --endpoints=http://10.100.0.10:2379,http://10.100.0.11:2379 put still running yes"
echo ""
echo "  # View boot log:"
echo "  $QARAX_BIN --server $SERVER vm console etcd-0"
echo ""
echo "  # Interactive serial console:"
echo "  $QARAX_BIN --server $SERVER vm attach etcd-0"
echo ""
echo "  # Tear down everything:"
echo "  ./demos/etcd-cluster/run.sh --cleanup"
