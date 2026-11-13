#!/usr/bin/env bash
#
# Demo: VPC network isolation and VM security groups
#
# Shows:
#   1. Two managed networks in one VPC, plus a third network in another VPC
#   2. Same-VPC routing between the two subnets (ping VM A -> VM B)
#   3. VPC isolation: VM A cannot reach VM C in the other VPC
#   4. An empty security group on VM B: default-deny ingress
#   5. An ICMP rule added live restores VM A -> VM B without restarting either VM
#
# With --cross-host, network A (with VMs A and C) lives on one host and network
# B (with VM B) on a second host, so same-VPC traffic crosses the VXLAN overlay.
#
# Prerequisites:
#   - local qarax stack (make run-local); started via hack/run-local.sh if not running
#   - --cross-host: a second UP host (on the local stack, qarax-node-2 is started
#     and registered automatically)
#   - jq, docker
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-netiso-"
VPC="${PREFIX}vpc"
OTHER_VPC="${PREFIX}other-vpc"
NET_A="${PREFIX}a"
NET_B="${PREFIX}b"
NET_C="${PREFIX}c"
SUBNET_A="10.121.1.0/24"
SUBNET_B="10.121.2.0/24"
SUBNET_C="10.122.1.0/24"
VM_A="${PREFIX}vm-a"
VM_B="${PREFIX}vm-b"
VM_C="${PREFIX}vm-c"
VM_A_IP="10.121.1.10"
VM_B_IP="10.121.2.10"
VM_C_IP="10.122.1.10"
SG="${PREFIX}sg"

CROSS_HOST=0
CLEANUP_ONLY=0

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --cross-host   Put network B (and VM B) on a second host
  --cleanup      Delete resources left by an earlier run, then exit
  --server URL   qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)
  --help, -h     Show this help

Environment:
  KEEP=1         Leave the VMs, networks and security group in place at exit
EOF
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--cross-host)
		CROSS_HOST=1
		shift
		;;
	--cleanup)
		CLEANUP_ONLY=1
		shift
		;;
	--server)
		SERVER="${2:?--server requires a URL}"
		shift 2
		;;
	--help | -h)
		usage
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

# Detach each demo network from every host, then delete it. Detaching tears
# down the bridge, DHCP server and NAT rules on the node; deleting an attached
# network would leave the bridge behind and the next run's attach would fail on
# the existing bridge name.
sweep_all() {
	sweep vm "$PREFIX"
	sweep security-group "$PREFIX"
	sweep network "$PREFIX"
}

cleanup() {
	if [[ "$KEEP" == 1 ]]; then
		info "KEEP=1: leaving ${PREFIX}* VMs, networks and security group. Remove them with: $0 --cleanup"
		return
	fi
	step "Cleaning up"
	sweep_all
}

# guest IP CMD...: run CMD in the guest at IP over SSH from the node container
# that hosts it (guest subnets are only routable from there). The output lands
# in GUEST_OUT so a polling caller can show the last attempt.
GUEST_OUT=""
guest() {
	local ip="$1"
	shift
	GUEST_OUT=$(node_exec "$NODE_A" timeout 20 dbclient -y -i /root/.ssh/id_rsa "root@${ip}" "$@" 2>&1)
}

# Each boot gives a guest new SSH host keys; forget the ones an earlier run
# recorded for the same IP.
forget_host_key() {
	node_exec "$NODE_A" sh -c 'f=/root/.ssh/known_hosts; [ ! -f "$f" ] || sed -i "/$1/d" "$f"' sh "${1//./\\.}"
}

# ping_ok FROM TO / ping_blocked FROM TO: ping TO from inside the guest at FROM.
# Blocked means ping ran and got no replies; an SSH failure is neither.
ping_ok() {
	guest "$1" ping -c 3 -W 1 "$2" && [[ "$GUEST_OUT" == *" 0% packet loss"* ]]
}
ping_blocked() {
	# ping exits non-zero when every probe is lost; its summary line decides.
	guest "$1" ping -c 3 -W 1 "$2" || true
	[[ "$GUEST_OUT" == *"100% packet loss"* ]]
}

# expect_ping ping_ok|ping_blocked FROM TO: poll until the ping has that outcome,
# then show the guest's ping output.
expect_ping() {
	local check="$1" from="$2" to="$3"
	info "\$ ping -c 3 -W 1 ${to}   (inside the guest at ${from})"
	if ! wait_until 60 "$check" "$from" "$to"; then
		echo "$GUEST_OUT" >&2
		die "ping ${from} -> ${to}: expected ${check#ping_}"
	fi
	sed 's/^/    /' <<<"$GUEST_OUT"
}

# assert_on_host VM HOST_ID HOST_NAME
assert_on_host() {
	local host_id
	host_id=$(qarax vm get "$1" -o json | jq -r '.host_id')
	[[ "$host_id" == "$2" ]] || die "${1} was scheduled onto host ${host_id}, expected ${3}"
	ok "${1} on ${3}"
}

banner "Network Isolation Demo"

command -v jq >/dev/null || die "jq is required"
ensure_stack
require_cli

if ((CLEANUP_ONLY)); then
	sweep_all
	ok "Removed ${PREFIX}* resources"
	exit 0
fi

step "Preparing"
HOST_COUNT=$((CROSS_HOST + 1))
ensure_up_hosts "$HOST_COUNT"
mapfile -t UP_HOSTS < <(qarax host list -o json |
	jq -r '.[] | select(.status == "up") | [.id, .name, .address] | @tsv')
# Without --cross-host, host B is host A.
IFS=$'\t' read -r HOST_A_ID HOST_A HOST_A_ADDR <<<"${UP_HOSTS[0]}"
IFS=$'\t' read -r HOST_B_ID HOST_B _ <<<"${UP_HOSTS[HOST_COUNT - 1]}"
NODE_A=$(node_service_for_address "$HOST_A_ADDR") ||
	die "host ${HOST_A} (${HOST_A_ADDR}) is not a local compose node: this demo reaches guests through the qarax-node container"
info "Host A: ${HOST_A} (networks ${NET_A}, ${NET_C})"
info "Host B: ${HOST_B} (network ${NET_B})"

sweep_all
taken=$(qarax network list -o json | jq -r --arg a "$SUBNET_A" --arg b "$SUBNET_B" --arg c "$SUBNET_C" \
	'[.[] | select(.subnet == $a or .subnet == $b or .subnet == $c) | .name] | join(", ")')
[[ -z "$taken" ]] || die "other networks already use this demo's subnets: ${taken}"
trap cleanup EXIT

# Type "isolated" (a bridge with NAT, like the default "bridge") is the type
# whose same-VPC traffic qarax exempts from NAT, so VM B sees VM A's own
# address and the security-group rule for subnet A below matches, also when
# the packets cross hosts.
step "Creating networks: ${NET_A} and ${NET_B} in VPC ${VPC}, ${NET_C} in VPC ${OTHER_VPC}"
run qarax network create --name "$NET_A" --subnet "$SUBNET_A" --gateway 10.121.1.1 --vpc "$VPC" --network-type isolated
run qarax network create --name "$NET_B" --subnet "$SUBNET_B" --gateway 10.121.2.1 --vpc "$VPC" --network-type isolated
run qarax network create --name "$NET_C" --subnet "$SUBNET_C" --gateway 10.122.1.1 --vpc "$OTHER_VPC" --network-type isolated

step "Attaching the networks to their hosts"
run qarax network attach-host --network "$NET_A" --host "$HOST_A" --bridge-name qnisoa
run qarax network attach-host --network "$NET_B" --host "$HOST_B" --bridge-name qnisob
run qarax network attach-host --network "$NET_C" --host "$HOST_A" --bridge-name qnisoc
run qarax network list

step "Creating and starting one VM per network"
run qarax vm create --name "$VM_A" --vcpus 1 --memory 256MiB --network "$NET_A" --ip "$VM_A_IP"
run qarax vm create --name "$VM_B" --vcpus 1 --memory 256MiB --network "$NET_B" --ip "$VM_B_IP"
run qarax vm create --name "$VM_C" --vcpus 1 --memory 256MiB --network "$NET_C" --ip "$VM_C_IP"
start_vms "$VM_A" "$VM_B" "$VM_C"
assert_on_host "$VM_A" "$HOST_A_ID" "$HOST_A"
assert_on_host "$VM_B" "$HOST_B_ID" "$HOST_B"
assert_on_host "$VM_C" "$HOST_A_ID" "$HOST_A"
forget_host_key "$VM_A_IP"
forget_host_key "$VM_C_IP"

step "Same VPC: ${VM_A} (${VM_A_IP}) reaches ${VM_B} (${VM_B_IP})"
expect_ping ping_ok "$VM_A_IP" "$VM_B_IP"

step "Different VPC: ${VM_A} cannot reach ${VM_C} (${VM_C_IP})"
wait_until 60 guest "$VM_C_IP" true || die "${VM_C} is not answering SSH from ${HOST_A}: ${GUEST_OUT}"
ok "${VM_C} is up and answers SSH from its host"
expect_ping ping_blocked "$VM_A_IP" "$VM_C_IP"

step "Attaching an empty security group to ${VM_B}"
run qarax security-group create --name "$SG" --description "default-deny ingress until ICMP is allowed"
run qarax vm attach-security-group "$VM_B" --security-group "$SG"
expect_ping ping_blocked "$VM_A_IP" "$VM_B_IP"

step "Allowing ICMP from ${SUBNET_A} (applied live, no VM restart)"
run qarax security-group add-rule --security-group "$SG" --direction ingress --protocol icmp \
	--cidr "$SUBNET_A" --description "allow pings from subnet A"
run qarax security-group list-rules "$SG"
expect_ping ping_ok "$VM_A_IP" "$VM_B_IP"

banner "Demo Complete"
ok "Same-VPC routing worked from ${NET_A} on ${HOST_A} to ${NET_B} on ${HOST_B}"
ok "${NET_C}, in another VPC on the same host as ${NET_A}, was unreachable from it"
ok "An empty security group blocked ingress to ${VM_B}; one ICMP rule restored it live"
