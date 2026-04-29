#!/usr/bin/env bash
#
# Demo: cross-host VPC routing plus live security-group updates
#
# Shows:
#   1. Two isolated networks in one VPC
#   2. Network A attached to host 1 and network B attached to host 2
#   3. One VM per network, scheduled onto different hosts
#   4. Cross-host connectivity working before security groups
#   5. An empty security group attached to VM B, causing default-deny ingress
#   6. A live ICMP rule update restoring connectivity without recreating the VM
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

cd "$REPO_ROOT"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
KEEP_RESOURCES=false
export CLOUD_HYPERVISOR_VERSION="${CLOUD_HYPERVISOR_VERSION:-$(tr -d '\n' < "${REPO_ROOT}/versions/cloud-hypervisor-version")}"
export FIRECRACKER_VERSION="${FIRECRACKER_VERSION:-$(tr -d '\n' < "${REPO_ROOT}/versions/firecracker-version")}"

SUFFIX="$$"
VPC_NAME="demo-xhost-vpc-${SUFFIX}"
NETWORK_A="demo-xhost-a-${SUFFIX}"
NETWORK_B="demo-xhost-b-${SUFFIX}"
SUBNET_A="10.121.1.0/24"
SUBNET_B="10.121.2.0/24"
GATEWAY_A="10.121.1.1"
GATEWAY_B="10.121.2.1"
VM_A_IP="10.121.1.10"
VM_B_IP="10.121.2.10"
VM_A="demo-xhost-vm-a-${SUFFIX}"
VM_B="demo-xhost-vm-b-${SUFFIX}"
SG_NAME="demo-xhost-sg-${SUFFIX}"
BRIDGE_A="qcha${SUFFIX}"
BRIDGE_B="qchb${SUFFIX}"
MEMORY_MIB=256
MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))

PRIMARY_HOST_ID=""
PRIMARY_HOST_NAME=""
PRIMARY_HOST_ADDRESS=""
PRIMARY_NODE_SERVICE=""
SECONDARY_HOST_ID=""
SECONDARY_HOST_NAME=""
SECONDARY_HOST_ADDRESS=""
SECONDARY_NODE_SERVICE=""
RULE_ID=""
SG_ATTACHED=false

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

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --server URL       qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)
  --keep-resources   Skip automatic cleanup so you can inspect the result
  --help, -h         Show this help
EOF
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--server)
		SERVER="$2"
		shift 2
		;;
	--keep-resources)
		KEEP_RESOURCES=true
		shift
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

if [[ -z "$(find_qarax_bin)" ]]; then
	echo "qarax CLI not found — building..."
	cargo build -p cli
fi

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found even after build"

qarax() {
	"$QARAX_BIN" --server "$SERVER" "$@"
}

docker_compose() {
	docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" "$@"
}

node_service_for_address() {
	case "$1" in
	qarax-node)
		echo "qarax-node"
		;;
	qarax-node-2)
		echo "qarax-node-2"
		;;
	*)
		return 1
		;;
	esac
}

wait_for_vm_status() {
	local vm="$1"
	local target="$2"
	local timeout="${3:-60}"
	local elapsed=0

	while [[ $elapsed -lt $timeout ]]; do
		local status
		status="$(qarax vm get "$vm" -o json | jq -r '.status')"
		if [[ "$status" == "$target" ]]; then
			info "${vm}: ${status}"
			return 0
		fi
		sleep 2
		elapsed=$((elapsed + 2))
	done

	die "Timed out waiting for VM ${vm} to reach status '${target}'"
}

clear_known_host() {
	docker_compose exec -T "$PRIMARY_NODE_SERVICE" sh -lc \
		"sed -i '/${VM_A_IP}/d' /root/.ssh/known_hosts 2>/dev/null || true"
}

ping_vm_b_from_vm_a() {
	local output status
	set +e
	output="$(
		docker_compose exec -T "$PRIMARY_NODE_SERVICE" \
			dbclient -y -i /root/.ssh/id_rsa "root@${VM_A_IP}" \
			ping -c 3 -W 1 "$VM_B_IP" 2>&1
	)"
	status=$?
	set -e

	printf '%s\n' "$output"
	return "$status"
}

wait_for_ping_state() {
	local desired="$1"
	local attempts="${2:-15}"
	local delay="${3:-2}"
	local last_output=""

	for _ in $(seq 1 "$attempts"); do
		set +e
		last_output="$(ping_vm_b_from_vm_a)"
		local status=$?
		set -e

		if [[ "$desired" == "allowed" ]]; then
			if [[ $status -eq 0 && "$last_output" == *"0% packet loss"* ]]; then
				info "Ping succeeded:"
				echo "$last_output"
				return 0
			fi
		else
			if [[ $status -ne 0 ]]; then
				info "Ping is blocked (non-zero exit is expected here):"
				echo "$last_output"
				return 0
			fi
		fi

		sleep "$delay"
	done

	if [[ "$desired" == "allowed" ]]; then
		die "Expected ping from ${VM_A} (${VM_A_IP}) to ${VM_B} (${VM_B_IP}) to succeed"
	fi
	die "Expected ping from ${VM_A} (${VM_A_IP}) to ${VM_B} (${VM_B_IP}) to be blocked"
}

cleanup() {
	if [[ "$KEEP_RESOURCES" == "true" ]]; then
		echo
		step "Keeping demo resources for inspection"
		info "VMs: ${VM_A}, ${VM_B}"
		info "Networks: ${NETWORK_A}, ${NETWORK_B}"
		info "Security group: ${SG_NAME}"
		return
	fi

	echo
	step "Cleaning up..."

	if [[ "$SG_ATTACHED" == "true" ]]; then
		qarax vm detach-security-group "$VM_B" --security-group "$SG_NAME" 2>/dev/null || true
	fi

	qarax vm stop "$VM_A" --wait 2>/dev/null || true
	qarax vm stop "$VM_B" --wait 2>/dev/null || true
	qarax vm delete "$VM_A" 2>/dev/null || true
	qarax vm delete "$VM_B" 2>/dev/null || true

	qarax security-group delete "$SG_NAME" 2>/dev/null || true

	if [[ -n "$PRIMARY_HOST_NAME" ]]; then
		qarax network detach-host --network "$NETWORK_A" --host "$PRIMARY_HOST_NAME" 2>/dev/null || true
	fi
	if [[ -n "$SECONDARY_HOST_NAME" ]]; then
		qarax network detach-host --network "$NETWORK_B" --host "$SECONDARY_HOST_NAME" 2>/dev/null || true
	fi

	qarax network delete "$NETWORK_A" 2>/dev/null || true
	qarax network delete "$NETWORK_B" 2>/dev/null || true

	info "Done."
}
trap cleanup EXIT

banner "Cross-Host VPC Security Group Demo"

step "Preflight checks"
command -v jq >/dev/null || die "jq is required"
command -v docker >/dev/null || die "docker is required"
docker compose version >/dev/null 2>&1 || die "docker compose is required"

ensure_stack "$SERVER"

host_json="$(qarax host list -o json 2>&1)" || {
	if grep -qi "missing field" <<<"$host_json"; then
		die "CLI/server schema mismatch detected. Rebuild the local stack with: REBUILD=1 ./hack/run-local.sh"
	fi
	die "Failed to list hosts: $host_json"
}

PRIMARY_HOST_ID="$(jq -r '([.[] | select(.status == "up" and (.name == "e2e-node-1" or .address == "qarax-node"))][0] // [.[] | select(.status == "up")][0]).id // empty' <<<"$host_json")"
PRIMARY_HOST_NAME="$(jq -r '([.[] | select(.status == "up" and (.name == "e2e-node-1" or .address == "qarax-node"))][0] // [.[] | select(.status == "up")][0]).name // empty' <<<"$host_json")"
PRIMARY_HOST_ADDRESS="$(jq -r '([.[] | select(.status == "up" and (.name == "e2e-node-1" or .address == "qarax-node"))][0] // [.[] | select(.status == "up")][0]).address // empty' <<<"$host_json")"
[[ -n "$PRIMARY_HOST_ID" ]] || die "No UP hosts available"

SECONDARY_HOST_ID="$(jq -r --arg primary "$PRIMARY_HOST_ID" '([.[] | select(.status == "up" and .id != $primary and (.name == "e2e-node-2" or .address == "qarax-node-2"))][0] // [.[] | select(.status == "up" and .id != $primary)][0]).id // empty' <<<"$host_json")"
SECONDARY_HOST_NAME="$(jq -r --arg primary "$PRIMARY_HOST_ID" '([.[] | select(.status == "up" and .id != $primary and (.name == "e2e-node-2" or .address == "qarax-node-2"))][0] // [.[] | select(.status == "up" and .id != $primary)][0]).name // empty' <<<"$host_json")"
SECONDARY_HOST_ADDRESS="$(jq -r --arg primary "$PRIMARY_HOST_ID" '([.[] | select(.status == "up" and .id != $primary and (.name == "e2e-node-2" or .address == "qarax-node-2"))][0] // [.[] | select(.status == "up" and .id != $primary)][0]).address // empty' <<<"$host_json")"
[[ -n "$SECONDARY_HOST_ID" ]] || die "This demo requires two UP hosts. Start the two-node e2e stack first."

PRIMARY_NODE_SERVICE="$(node_service_for_address "$PRIMARY_HOST_ADDRESS")" || die "Unsupported primary host address '${PRIMARY_HOST_ADDRESS}'"
SECONDARY_NODE_SERVICE="$(node_service_for_address "$SECONDARY_HOST_ADDRESS")" || die "Unsupported secondary host address '${SECONDARY_HOST_ADDRESS}'"

info "Primary host:   ${PRIMARY_HOST_NAME} (${PRIMARY_HOST_ADDRESS})"
info "Secondary host: ${SECONDARY_HOST_NAME} (${SECONDARY_HOST_ADDRESS})"
info "SSH hop service: ${PRIMARY_NODE_SERVICE}"
info "Resources are named with suffix: ${SUFFIX}"

step "Creating two isolated networks in the same VPC"
run qarax network create --name "$NETWORK_A" --subnet "$SUBNET_A" --gateway "$GATEWAY_A" --vpc "$VPC_NAME" --network-type isolated
run qarax network create --name "$NETWORK_B" --subnet "$SUBNET_B" --gateway "$GATEWAY_B" --vpc "$VPC_NAME" --network-type isolated

step "Attaching network A to ${PRIMARY_HOST_NAME} and network B to ${SECONDARY_HOST_NAME}"
run qarax network attach-host --network "$NETWORK_A" --host "$PRIMARY_HOST_NAME" --bridge-name "$BRIDGE_A"
run qarax network attach-host --network "$NETWORK_B" --host "$SECONDARY_HOST_NAME" --bridge-name "$BRIDGE_B"

step "Inspecting the managed networks and their shared VPC"
run qarax network get "$NETWORK_A"
echo
run qarax network get "$NETWORK_B"
echo

step "Creating one VM on each network"
run qarax vm create --name "$VM_A" --vcpus 1 --memory "$MEMORY_BYTES" --network "$NETWORK_A" --ip "$VM_A_IP"
run qarax vm create --name "$VM_B" --vcpus 1 --memory "$MEMORY_BYTES" --network "$NETWORK_B" --ip "$VM_B_IP"

step "Starting both VMs"
run qarax vm start "$VM_A"
run qarax vm start "$VM_B"

step "Waiting for both VMs to report running"
wait_for_vm_status "$VM_A" running 90
wait_for_vm_status "$VM_B" running 90
clear_known_host

step "Proving the VMs landed on different hosts"
VM_A_HOST_ID="$(qarax vm get "$VM_A" -o json | jq -r '.host_id // empty')"
VM_B_HOST_ID="$(qarax vm get "$VM_B" -o json | jq -r '.host_id // empty')"
[[ "$VM_A_HOST_ID" == "$PRIMARY_HOST_ID" ]] || die "${VM_A} did not land on ${PRIMARY_HOST_NAME}"
[[ "$VM_B_HOST_ID" == "$SECONDARY_HOST_ID" ]] || die "${VM_B} did not land on ${SECONDARY_HOST_NAME}"
info "${VM_A} -> ${PRIMARY_HOST_NAME}"
info "${VM_B} -> ${SECONDARY_HOST_NAME}"

step "Showing initial cross-host same-VPC connectivity"
info "VM ${VM_A} will SSH from ${PRIMARY_NODE_SERVICE} and ping ${VM_B_IP} on ${SECONDARY_HOST_NAME}"
wait_for_ping_state allowed 20 2

step "Creating an empty security group and attaching it to ${VM_B}"
run qarax security-group create --name "$SG_NAME" --description "default-deny ingress until ICMP is allowed"
run qarax vm attach-security-group "$VM_B" --security-group "$SG_NAME"
SG_ATTACHED=true
run qarax vm list-security-groups "$VM_B"
echo

step "Demonstrating that the empty security group blocks ingress"
info "A timeout or non-zero exit counts as blocked behavior here."
wait_for_ping_state blocked 15 2

step "Adding an ICMP ingress rule for subnet ${SUBNET_A}"
run qarax security-group add-rule \
	--security-group "$SG_NAME" \
	--direction ingress \
	--protocol icmp \
	--cidr "$SUBNET_A" \
	--description "allow pings from subnet A"
RULE_ID="$(qarax security-group list-rules "$SG_NAME" -o json | jq -r '.[-1].id // empty')"
[[ -n "$RULE_ID" ]] && info "Rule ID: ${RULE_ID}"
run qarax security-group list-rules "$SG_NAME"
echo

step "Showing live firewall sync after the rule update"
info "The VMs are not restarted here; the rule is applied live across hosts."
wait_for_ping_state allowed 20 2

banner "Demo Complete"
info "Cross-host same-VPC routing worked between ${PRIMARY_HOST_NAME} and ${SECONDARY_HOST_NAME}."
info "Attaching an empty security group blocked ingress to ${VM_B}."
info "Adding one ICMP rule restored connectivity immediately."
