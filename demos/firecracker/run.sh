#!/usr/bin/env bash
#
# Demo: Firecracker backend integration
#
# Exercises a real Firecracker VM lifecycle end-to-end:
#   1) Create VM with --hypervisor firecracker
#   2) Start VM
#   3) Pause VM
#   4) Resume VM
#   5) Stop VM
#   6) Delete VM (cleanup)
#
# Prerequisites:
#   - qarax stack running (make run-local), or let the script start it
#   - Firecracker on qarax-node (the local qarax-node image ships it)
#
# The script builds the qarax CLI first; set SKIP_BUILD=1 to use an existing
# binary (target/ or PATH).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
VM_NAME="fc-demo-$(date +%s)-$$"
VCPUS=1
MEMORY_MIB=128
NO_CLEANUP=0

while [[ $# -gt 0 ]]; do
	case "$1" in
	--server)
		SERVER="$2"
		shift 2
		;;
	--name)
		VM_NAME="$2"
		shift 2
		;;
	--vcpus)
		VCPUS="$2"
		shift 2
		;;
	--memory)
		MEMORY_MIB="$2"
		shift 2
		;;
	--no-cleanup)
		NO_CLEANUP=1
		shift
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo ""
		echo "Options:"
		echo "  --server URL      qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --name NAME       VM name (default: fc-demo-<timestamp>)"
		echo "  --vcpus N         vCPU count (default: 1)"
		echo "  --memory MiB      memory in MiB (default: 128)"
		echo "  --no-cleanup      leave VM after demo"
		exit 0
		;;
	*)
		echo "Unknown option: $1" >&2
		exit 1
		;;
	esac
done

MEMORY_BYTES=$((MEMORY_MIB * 1024 * 1024))

if [[ -z "${SKIP_BUILD:-}" ]]; then
	echo -e "${GREEN}▸${NC} Building qarax CLI..."
	cargo build -p cli >/dev/null
fi

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found"
QARAX="$QARAX_BIN --server $SERVER"

cleanup() {
	if [[ "$NO_CLEANUP" -eq 1 ]]; then
		return 0
	fi
	$QARAX vm force-stop "$VM_NAME" >/dev/null 2>&1 || true
	$QARAX vm delete "$VM_NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

json_status() {
	python3 -c 'import json,sys; print(json.load(sys.stdin).get("status",""))'
}

wait_for_status() {
	local vm="$1"
	local target="$2"
	local timeout="${3:-60}"
	local elapsed=0
	local current

	while ((elapsed < timeout)); do
		if ! current=$($QARAX vm get "$vm" -o json 2>/dev/null | json_status); then
			current="unknown"
		fi
		if [[ "$current" == "$target" ]]; then
			echo -e "  ${GREEN}✓${NC} status=${current}"
			return 0
		fi
		echo -ne "  ${DIM}waiting for ${target} (current=${current}, ${elapsed}s/${timeout}s)${NC}\r"
		sleep 2
		elapsed=$((elapsed + 2))
	done
	echo ""
	$QARAX vm get "$vm" >&2 || true
	die "Timed out waiting for VM '${vm}' to reach status '${target}' (last status: ${current}).\nCheck the node logs: docker compose -f e2e/docker-compose.yml logs qarax-node"
}

echo -e "${BOLD}${CYAN}=== Firecracker Integration Demo ===${NC}"
ensure_stack "$SERVER"

echo -e "${GREEN}▸${NC} Checking for a Firecracker-capable host..."
FC_HOSTS=$($QARAX host list -o json | python3 -c 'import json,sys; print(sum(1 for h in json.load(sys.stdin) if h.get("status","").lower() == "up" and h.get("firecracker_version")))')
if [[ "$FC_HOSTS" -eq 0 ]]; then
	echo -e "  ${YELLOW}No 'up' host reports a Firecracker version; VM creation may fail.${NC}"
fi

echo -e "${GREEN}▸${NC} Creating Firecracker VM: ${BOLD}${VM_NAME}${NC}"
if ! $QARAX vm create \
	--name "$VM_NAME" \
	--hypervisor firecracker \
	--vcpus "$VCPUS" \
	--memory "$MEMORY_BYTES" >/dev/null; then
	die "Failed to create Firecracker VM. Ensure qarax-node has /usr/local/bin/firecracker and KVM access."
fi

wait_for_status "$VM_NAME" "created" 30

echo -e "${GREEN}▸${NC} Starting VM"
$QARAX vm start "$VM_NAME" >/dev/null
wait_for_status "$VM_NAME" "running" 60

echo -e "${GREEN}▸${NC} Pausing VM"
$QARAX vm pause "$VM_NAME" >/dev/null
wait_for_status "$VM_NAME" "paused" 30

echo -e "${GREEN}▸${NC} Resuming VM"
$QARAX vm resume "$VM_NAME" >/dev/null
wait_for_status "$VM_NAME" "running" 30

echo -e "${GREEN}▸${NC} Stopping VM"
$QARAX vm stop "$VM_NAME" >/dev/null
wait_for_status "$VM_NAME" "shutdown" 60

echo -e "${GREEN}▸${NC} Final VM state"
$QARAX vm get "$VM_NAME"

if [[ "$NO_CLEANUP" -eq 1 ]]; then
	echo -e "${YELLOW}Leaving VM '${VM_NAME}' as requested (--no-cleanup).${NC}"
else
	echo -e "${GREEN}▸${NC} Cleaning up VM"
	$QARAX vm delete "$VM_NAME" >/dev/null || true
	trap - EXIT
fi

echo -e "${GREEN}Demo complete.${NC}"
