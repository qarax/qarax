#!/usr/bin/env bash
#
# Demo: a 3-node etcd cluster on qarax VMs
#
# Builds the etcd node image (Containerfile in this directory), pushes it to
# the local stack's registry, imports it into overlaybd-pool (which converts
# it to OverlayBD) and boots three VMs from it with static IPs on a qarax
# network. It then checks that the members formed a cluster, writes a key
# through one node and reads it through another, and plumbs a veth from this
# machine into the VM bridge (sudo) so the cluster is reachable from here.
#
# The cluster is left running. A re-run replaces the VMs and the image object
# and reuses the network; --cleanup removes all of it.
#
# Layout (NET_PREFIX below is the one place to change the subnet):
#   etcd-net   10.100.0.0/24, gateway 10.100.0.1 on bridge br-etcd
#   etcd-0     10.100.0.10
#   etcd-1     10.100.0.11
#   etcd-2     10.100.0.12
#   this host  10.100.0.254 on veth-etcd
#
# Needs the local Docker Compose stack (make run-local, not VM=1): the VM
# bridge lives in the qarax-node container's network namespace.
#
# Requirements: docker, jq, curl, sudo, KVM; cargo or a qarax CLI on PATH.
#
# Usage:
#   ./demos/etcd-cluster/run.sh            # build, boot and check the cluster
#   ./demos/etcd-cluster/run.sh --cleanup  # remove the VMs, network, image object and host veth
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEMO_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

NET_PREFIX="10.100.0"
NETWORK="etcd-net"
SUBNET="${NET_PREFIX}.0/24"
GATEWAY="${NET_PREFIX}.1"
BRIDGE="br-etcd"
HOST_VETH="veth-etcd"
HOST_ACCESS_CIDR="${NET_PREFIX}.254/24"
POOL="overlaybd-pool"
REPO="etcd-node"         # repository in the local registry
IMAGE_OBJECT="etcd-node" # storage object the VMs boot from
NODES=(etcd-0 etcd-1 etcd-2)
NODE_IPS=("${NET_PREFIX}.10" "${NET_PREFIX}.11" "${NET_PREFIX}.12")
MEMORY="256MiB"
BOOT_TIMEOUT=180

members=() endpoints=()
for i in "${!NODES[@]}"; do
	members+=("${NODES[$i]}=http://${NODE_IPS[$i]}:2380")
	endpoints+=("http://${NODE_IPS[$i]}:2379")
done
CLUSTER_MEMBERS=$(
	IFS=,
	echo "${members[*]}"
)
ENDPOINTS=$(
	IFS=,
	echo "${endpoints[*]}"
)

CLEANUP=0
while [[ $# -gt 0 ]]; do
	case "$1" in
	--cleanup)
		CLEANUP=1
		shift
		;;
	*)
		die "unknown option: $1 (usage: $0 [--cleanup])"
		;;
	esac
done

# Set HOST (the UP host the demo uses) and NODE_CID (the compose container
# that runs it).
resolve_node() {
	local address service
	# A host that can mount the OverlayBD image: one the shared pool is
	# attached to (nodes without overlaybd-tcmu refuse it), and UP.
	HOST=$(qarax host list -o json | jq -r --argjson attached "$(qarax storage-pool hosts "$POOL" -o json)" \
		'[.[] | select(.status == "up" and (.name as $n | $attached | any(.host_name == $n)))][0].name // empty')
	[[ -n "$HOST" ]] || die "no UP host has storage pool ${POOL}; run 'make run-local'"
	address=$(qarax host get "$HOST" -o json | jq -r .address)
	service=$(node_service_for_address "$address") ||
		die "host ${HOST} (${address}) is not a local compose node: this demo needs the Docker Compose stack (make run-local, without VM=1)"
	NODE_CID=$(compose ps -q "$service")
	[[ -n "$NODE_CID" ]] || die "compose service ${service} is not running"
}

# Delete this demo's VMs by name (etcd-* is too generic to sweep by prefix).
# Deleting a running VM stops it on the node.
delete_vms() {
	local vm
	for vm in "${NODES[@]}"; do
		if exists vm "$vm"; then
			run qarax vm delete "$vm"
		fi
	done
}

# Whether this machine has the veth into the VM bridge; "no" is an error from ip.
host_veth_up() {
	ip link show "$HOST_VETH" >/dev/null 2>&1
}

# plumb_host_veth CONTAINER BRIDGE VETH CIDR: connect this machine to BRIDGE
# in CONTAINER's network namespace with a veth pair (VETH here, VETH-br in the
# container) and give the host end CIDR, so VMs on that bridge are reachable
# from here. demos/k8s-cluster does the same.
plumb_host_veth() {
	local container="$1" bridge="$2" veth="$3" cidr="$4" pid
	pid=$(docker inspect -f '{{.State.Pid}}' "$container")
	run sudo ip link add "$veth" type veth peer name "${veth}-br"
	run sudo ip link set "${veth}-br" netns "$pid"
	run sudo nsenter -t "$pid" -n ip link set "${veth}-br" master "$bridge" up
	run sudo ip addr add "$cidr" dev "$veth"
	run sudo ip link set "$veth" up
}

# `vm exec` fails (HTTP 422) until the guest agent is up, and etcdctl fails
# until the members have elected a leader: both are expected while the VMs boot.
cluster_healthy() {
	qarax vm exec "${NODES[0]}" -- etcdctl --endpoints="$ENDPOINTS" endpoint health >/dev/null 2>&1
}

banner "qarax demo: 3-node etcd cluster"

if ((CLEANUP)); then
	step "Removing the etcd demo"
	if host_veth_up; then
		run sudo ip link del "$HOST_VETH"
	fi
	if ! server_reachable; then
		info "qarax server not reachable at ${SERVER}: nothing else to clean up."
		exit 0
	fi
	require_auth
	require_cli
	delete_vms
	if exists storage-object "$IMAGE_OBJECT"; then
		run qarax storage-object delete "$IMAGE_OBJECT"
	fi
	if exists network "$NETWORK"; then
		# Also detaches it from the node: bridge, DHCP server and NAT rules.
		run qarax network delete "$NETWORK"
	fi
	info "The ${REPO} images stay in the local registry (its volume goes with the stack)."
	ok "Done"
	exit 0
fi

for tool in docker jq curl sudo; do
	command -v "$tool" >/dev/null || die "${tool} is required"
done

ensure_stack
require_cli
resolve_node
exists storage-pool "$POOL" ||
	die "storage pool ${POOL} not found: make run-local creates it"

info "Host:    ${HOST}"
info "Network: ${NETWORK} (${SUBNET})"
info "Nodes:   ${NODES[*]}"

step "Remove VMs left by an earlier run"
delete_vms

step "1. Build the etcd node image and push it to the local registry"
run docker build -f "${DEMO_DIR}/Containerfile" --build-arg "CLUSTER_MEMBERS=${CLUSTER_MEMBERS}" \
	-t "localhost:5001/${REPO}:src" "$DEMO_DIR"
run docker push "localhost:5001/${REPO}:src"

step "2. Network ${NETWORK} on ${HOST}"
if exists network "$NETWORK"; then
	ok "Network ${NETWORK} exists"
else
	run qarax network create --name "$NETWORK" --subnet "$SUBNET" --gateway "$GATEWAY"
fi
# Pin it to HOST: the members talk over the bridge, so they must share a host.
pin_network "$NETWORK" "$HOST" "$BRIDGE"

step "3. Import ${REPO}:src into ${POOL} (converts it to OverlayBD)"
# The import converts the image under its own tag, and step 1 just pushed a
# plain build over that tag, so import again on every run.
if exists storage-object "$IMAGE_OBJECT"; then
	run qarax storage-object delete "$IMAGE_OBJECT"
fi
run qarax storage-pool import --pool "$POOL" --image-ref "registry:5000/${REPO}:src" --name "$IMAGE_OBJECT"

step "4. Create the VMs (static IPs; the guest agent enables qarax vm exec)"
for i in "${!NODES[@]}"; do
	run qarax vm create --name "${NODES[$i]}" --vcpus 1 --memory "$MEMORY" \
		--network "$NETWORK" --ip "${NODE_IPS[$i]}" --guest-agent
	run qarax vm attach-disk "${NODES[$i]}" --object "$IMAGE_OBJECT"
done

step "5. Start the cluster"
start_vms "${NODES[@]}"

step "6. Wait for the members to elect a leader"
wait_until "$BOOT_TIMEOUT" cluster_healthy ||
	die "etcd did not become healthy within ${BOOT_TIMEOUT}s.\nBoot log: qarax vm console ${NODES[0]}"
run qarax vm exec "${NODES[0]}" -- etcdctl --endpoints="$ENDPOINTS" endpoint status -w table

step "7. Write through ${NODES[0]}, read through ${NODES[2]}"
expected="hello-from-${NODES[0]}"
run qarax vm exec "${NODES[0]}" -- etcdctl put greeting "$expected"
info "\$ qarax vm exec ${NODES[2]} -- etcdctl get --print-value-only greeting"
value=$(qarax vm exec "${NODES[2]}" -- etcdctl get --print-value-only greeting)
echo "$value"
[[ "$value" == "$expected" ]] || die "${NODES[2]} read '${value}', expected '${expected}'"
ok "The write through ${NODES[0]} is visible through ${NODES[2]}"

step "8. Reach the cluster from this machine (sudo)"
host_reachable=1
if host_veth_up; then
	ok "${HOST_VETH} exists"
elif sudo -n true 2>/dev/null || [[ -t 0 ]]; then
	plumb_host_veth "$NODE_CID" "$BRIDGE" "$HOST_VETH" "$HOST_ACCESS_CIDR"
else
	# No terminal to ask for a sudo password (e.g. CI): the cluster itself was
	# proven through vm exec above.
	host_reachable=0
	warn "Skipped: creating ${HOST_VETH} needs sudo, which can't prompt here. Re-run from a terminal to reach the members from this machine."
fi
if ((host_reachable)); then
	for i in "${!NODES[@]}"; do
		url="http://${NODE_IPS[$i]}:2379/health"
		health=$(curl -sf --max-time 5 "$url" | jq -r .health) ||
			die "${url} is not reachable from this machine through ${HOST_VETH}"
		[[ "$health" == true ]] || die "${NODES[$i]} reports health=${health}"
		ok "${NODES[$i]}: ${url} reports healthy"
	done
fi

banner "etcd cluster is ready"
echo "Members:"
for i in "${!NODES[@]}"; do
	echo "  ${NODES[$i]}  http://${NODE_IPS[$i]}:2379"
done
echo ""
echo "Try it (the qarax CLI needs: export QARAX_TOKEN=${QARAX_TOKEN}):"
echo ""
echo "  # etcdctl inside a member, no local install needed:"
echo "  qarax vm exec ${NODES[0]} -- etcdctl --endpoints=${ENDPOINTS} endpoint health"
echo "  qarax vm exec ${NODES[1]} -- etcdctl get greeting"
echo ""
echo "  # From this machine, over ${HOST_VETH}:"
echo "  curl http://${NODE_IPS[0]}:2379/version"
echo "  etcdctl --endpoints=${ENDPOINTS} member list   # if etcdctl is installed here"
echo ""
echo "  # Kill a member; the other two keep quorum:"
echo "  qarax vm force-stop ${NODES[2]}"
echo "  qarax vm exec ${NODES[0]} -- etcdctl put still-running yes"
echo ""
echo "  # etcd's log on a member's console:"
echo "  qarax vm console ${NODES[0]}"
echo ""
echo "  # Remove the VMs, network, image object and host veth:"
echo "  $0 --cleanup"
