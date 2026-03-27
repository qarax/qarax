#!/usr/bin/env bash
#
# Heap profiling: builds qarax + qarax-node with dhat instrumentation,
# runs the docker-compose stack, exercises key code paths, then collects
# dhat-heap.json profiles from both services.
#
# Usage:
#   ./scripts/profile-heap.sh
#   SKIP_BUILD=1 ./scripts/profile-heap.sh    # skip cargo + docker image build
#
# Outputs:
#   heap-profiles/dhat-heap-qarax.json
#   heap-profiles/dhat-heap-qarax-node.json
#
# View profiles at: https://nnethercote.github.io/dh_view/dh_view.html

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMPOSE_FILE="${REPO_ROOT}/e2e/docker-compose.yml"
PROFILE_DIR="${REPO_ROOT}/heap-profiles"
QARAX_URL="http://localhost:8000"
SERVICES=(postgres registry qarax-node qarax)

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'
die() {
	echo -e "${RED}ERROR: $*${NC}" >&2
	exit 1
}
info() { echo -e "${YELLOW}$*${NC}"; }
ok() { echo -e "${GREEN}$*${NC}"; }

mkdir -p "$PROFILE_DIR"

collect_profiles() {
	info "--- Stopping stack (SIGTERM → dhat writes profiles) ---"
	docker compose -f "$COMPOSE_FILE" stop "${SERVICES[@]}" 2>/dev/null || true
	sleep 2

	info "--- Collecting profiles ---"
	for entry in "qarax:/app/dhat-heap.json:dhat-heap-qarax.json" \
		"qarax-node:/dhat-heap.json:dhat-heap-qarax-node.json"; do
		svc="${entry%%:*}"
		rest="${entry#*:}"
		src="${rest%%:*}"
		dst="${rest#*:}"
		out="$PROFILE_DIR/$dst"
		if docker compose -f "$COMPOSE_FILE" cp "$svc:$src" "$out" 2>/dev/null; then
			ok "  $svc: $out ($(du -h "$out" | cut -f1))"
		else
			echo "  WARNING: $svc profile not found (no file at $src)"
		fi
	done
}

cleanup() {
	collect_profiles
	docker compose -f "$COMPOSE_FILE" down --volumes --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT

if [[ -z "${SKIP_BUILD:-}" ]]; then
	info "--- Building release binaries with dhat-heap feature + debug symbols ---"
	RUSTFLAGS="-g" cargo build --release -p qarax -p qarax-node \
		--features qarax/dhat-heap,qarax-node/dhat-heap

	info "--- Building docker images ---"
	docker compose -f "$COMPOSE_FILE" build qarax qarax-node
else
	info "Skipping build (SKIP_BUILD=1)"
fi

info "--- Starting stack ---"
docker compose -f "$COMPOSE_FILE" up -d "${SERVICES[@]}"

info "--- Waiting for qarax API ---"
elapsed=0
until curl -sf "$QARAX_URL/" -o /dev/null 2>/dev/null; do
	sleep 2
	elapsed=$((elapsed + 2))
	[[ $elapsed -lt 120 ]] || die "Timeout waiting for qarax API"
	echo -n "."
done
echo ""
ok "qarax API ready"

info "--- Registering host ---"
# qarax-node is reachable by service name inside the docker network
HOST_ID=$(curl -sf -X POST "$QARAX_URL/hosts" \
	-H "Content-Type: application/json" \
	-d '{"name":"profile-node","address":"qarax-node","port":50051,"host_user":"root","password":""}' |
	jq -r '.[1]')
[[ -n "$HOST_ID" ]] || die "Failed to register host"
echo "Host: $HOST_ID"

info "--- Initializing host (exercises gRPC path) ---"
curl -sf -X POST "$QARAX_URL/hosts/$HOST_ID/init" -o /dev/null
ok "Host initialized"

info "--- Creating 20 VMs (exercises DB writes + JSON serialization) ---"
VM_IDS=()
for i in $(seq 1 20); do
	VM_ID=$(curl -sf -X POST "$QARAX_URL/vms" \
		-H "Content-Type: application/json" \
		-d "{\"name\":\"profile-vm-$i\",\"hypervisor\":\"cloud_hv\",\"boot_vcpus\":1,\"max_vcpus\":1,\"memory_size\":268435456}" |
		jq -r .)
	VM_IDS+=("$VM_ID")
done
ok "Created ${#VM_IDS[@]} VMs"

info "--- Reading VM list 20 times (exercises DB reads + serialization) ---"
for _ in $(seq 1 20); do
	curl -sf "$QARAX_URL/vms" -o /dev/null
done

info "--- Fetching each VM individually ---"
for VM_ID in "${VM_IDS[@]}"; do
	curl -sf "$QARAX_URL/vms/$VM_ID" -o /dev/null
done

info "--- Listing hosts 5 times ---"
for _ in $(seq 1 5); do
	curl -sf "$QARAX_URL/hosts" -o /dev/null
done

info "--- Deleting VMs ---"
for VM_ID in "${VM_IDS[@]}"; do
	curl -sf -X DELETE "$QARAX_URL/vms/$VM_ID" -o /dev/null
done
ok "Workload done — cleanup trap will stop stack and collect profiles"
