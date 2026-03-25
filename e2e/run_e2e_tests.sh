#!/usr/bin/env bash

set -e

echo "===== Qarax E2E Test Runner ====="

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Navigate to E2E directory
cd "$(dirname "$0")"

# Environment variables:
#   REBUILD=1       - Force rebuild of all images
#   KEEP=1          - Keep services running after tests (for debugging)
#   SKIP_BUILD=1    - Skip building qarax-node binary

default_webhook_host() {
	if [ "$(uname -s)" != "Linux" ]; then
		echo "host.docker.internal"
		return
	fi

	local gw
	gw=$(docker network inspect e2e_default 2>/dev/null |
		python3 -c "import json,sys; cfg=json.load(sys.stdin)[0]['IPAM']['Config']; print(cfg[0]['Gateway'])" 2>/dev/null) ||
		gw=$(docker network inspect bridge 2>/dev/null |
			python3 -c "import json,sys; cfg=json.load(sys.stdin)[0]['IPAM']['Config']; print(cfg[0]['Gateway'])" 2>/dev/null) ||
		gw="172.17.0.1"
	echo "$gw"
}

cleanup() {
	if [ -n "$KEEP" ]; then
		echo ""
		echo -e "${GREEN}Services kept running (KEEP=1)${NC}"
		echo ""
		echo "Useful commands:"
		echo "  docker compose logs -f           # Follow logs"
		echo "  docker compose logs qarax-node   # qarax-node logs"
		echo "  docker compose exec qarax-node sh  # Shell into qarax-node"
		echo "  docker compose down -v           # Stop and remove everything"
		echo ""
		echo "Test endpoint:"
		echo "  curl http://localhost:8000/vms"
		echo ""
	else
		echo -e "${YELLOW}Cleaning up...${NC}"
		rm -f "${BOOTC_OVERLAY_DISK}"
		docker compose down -v
	fi
}

trap cleanup EXIT

# Build qarax binaries if needed (Linux musl binary for Docker)
MUSL_TARGET="x86_64-unknown-linux-musl"
BOOTC_OVERLAY_DISK="${PWD}/bootc-vm-overlay.qcow2"
NODE_BINARY="../target/${MUSL_TARGET}/release/qarax-node"
if [ -z "$SKIP_BUILD" ]; then
	INIT_BINARY="../target/${MUSL_TARGET}/release/qarax-init"
	QARAX_BINARY="../target/${MUSL_TARGET}/release/qarax-server"
	CLI_BINARY="../target/${MUSL_TARGET}/release/qarax"
	if [ -n "$REBUILD" ] || [ ! -f "$NODE_BINARY" ] || [ ! -f "$INIT_BINARY" ] || [ ! -f "$QARAX_BINARY" ] || [ ! -f "$CLI_BINARY" ]; then
		echo -e "${YELLOW}Building qarax-server, qarax-node, qarax-init, and qarax CLI binaries...${NC}"
		cd ..
		if [ "$(uname -s)" = "Darwin" ]; then
			if ! command -v cross &>/dev/null; then
				echo -e "${RED}Cross-compilation from macOS requires 'cross'. Install with: cargo install cross${NC}"
				exit 1
			fi
			cross build --target "${MUSL_TARGET}" --release -p qarax -p qarax-node -p qarax-init
			cross build --target "${MUSL_TARGET}" --release -p cli
		else
			# If running under sudo, build as the original user so target/ stays user-owned.
			if [ -n "${SUDO_USER:-}" ]; then
				sudo -u "$SUDO_USER" cargo build --release -p qarax -p qarax-node -p qarax-init
				sudo -u "$SUDO_USER" cargo build --release -p cli
			else
				cargo build --release -p qarax -p qarax-node -p qarax-init
				cargo build --release -p cli
			fi
		fi
		cd e2e
	else
		echo -e "${GREEN}Using existing binaries${NC}"
		echo -e "${YELLOW}To rebuild, run: REBUILD=1 ./run_e2e_tests.sh${NC}"
	fi
fi

# ── Bootc VM disk (real bootc upgrade e2e tests) ─────────────────────────────
BOOTC_BASE_DISK="${PWD}/bootc-vm-base.qcow2"
NODE_BIN="../target/${MUSL_TARGET}/release/qarax-node"

build_bootc_vm() {
	# Ensure qemu-img is available (needed for overlay creation)
	if ! command -v qemu-img &>/dev/null; then
		if command -v apt-get &>/dev/null; then
			echo -e "${YELLOW}Installing qemu-utils...${NC}"
			sudo apt-get install -y -q qemu-utils
		else
			echo -e "${RED}qemu-img not found. Install qemu-utils.${NC}" >&2
			exit 1
		fi
	fi

	# Start just the registry so we can push images for BIB and the test
	echo -e "${YELLOW}Starting registry for bootc image build...${NC}"
	docker compose up -d registry
	local i
	for i in $(seq 1 30); do
		if docker compose ps registry 2>/dev/null | grep -q "(healthy)"; then
			break
		fi
		sleep 2
	done
	if ! docker compose ps registry 2>/dev/null | grep -q "(healthy)"; then
		echo -e "${RED}Registry failed to become healthy${NC}" >&2
		exit 1
	fi

	# Build base bootc image
	echo -e "${YELLOW}Building bootc node image...${NC}"
	local node_hash
	node_hash=$(sha256sum "${NODE_BIN}" | cut -d' ' -f1)
	docker build \
		--build-arg "CACHE_BUST=${node_hash}" \
		-f Containerfile.bootc-node \
		-t localhost:5001/qarax-node-bootc:base \
		..
	docker push localhost:5001/qarax-node-bootc:base

	# Push thin versioned images for bootc switch version tracking
	echo -e "${YELLOW}Pushing versioned bootc node images...${NC}"
	docker build -t localhost:5001/qarax-node-test:0.1.0 - <<'VEOF'
FROM localhost:5001/qarax-node-bootc:base
RUN mkdir -p /etc/qarax-node && printf 'QARAX_NODE_VERSION=0.1.0\n' > /etc/qarax-node/version.env
VEOF
	docker push localhost:5001/qarax-node-test:0.1.0

	docker build -t localhost:5001/qarax-node-test:0.2.0-test - <<'VEOF'
FROM localhost:5001/qarax-node-bootc:base
RUN mkdir -p /etc/qarax-node && printf 'QARAX_NODE_VERSION=0.2.0-test\n' > /etc/qarax-node/version.env
VEOF
	docker push localhost:5001/qarax-node-test:0.2.0-test

	# Build qcow2 disk from the bootc image using bootc-image-builder
	if [ -n "$REBUILD" ] || [ ! -f "$BOOTC_BASE_DISK" ]; then
		echo -e "${YELLOW}Building bootc VM disk (this takes a few minutes)...${NC}"
		local bib_output
		bib_output="${PWD}/bib-output"
		mkdir -p "${bib_output}"

		# BIB no longer pulls images itself; pre-pull into podman's local
		# container storage (shared with BIB via the bind-mount below).
		echo -e "${YELLOW}Pulling bootc image into local container storage...${NC}"
		sudo podman pull --tls-verify=false localhost:5001/qarax-node-bootc:base
		sudo podman tag localhost:5001/qarax-node-bootc:base registry:5000/qarax-node-bootc:base

		docker run --rm --privileged \
			--network e2e_default \
			-v "${bib_output}:/output" \
			-v /var/lib/containers/storage:/var/lib/containers/storage \
			quay.io/centos-bootc/bootc-image-builder:latest \
			--type qcow2 \
			registry:5000/qarax-node-bootc:base

		sudo mv "${bib_output}/qcow2/disk.qcow2" "${BOOTC_BASE_DISK}"
		sudo rm -rf "${bib_output}"
		echo -e "${GREEN}Bootc disk ready: ${BOOTC_BASE_DISK}${NC}"
	else
		echo -e "${GREEN}Using cached bootc disk: ${BOOTC_BASE_DISK}${NC}"
	fi

	# Fresh overlay for this run (keeps base unmodified for caching)
	# Use the basename as the backing file so the path stays valid inside
	# the container (both disks are mounted at /disk/).
	qemu-img create -f qcow2 -F qcow2 \
		-b "$(basename "${BOOTC_BASE_DISK}")" \
		"${BOOTC_OVERLAY_DISK}" >/dev/null
	echo -e "${GREEN}Bootc VM overlay created.${NC}"
}

build_bootc_vm

# Build and start services
echo -e "${YELLOW}Starting services...${NC}"
if [ -n "$REBUILD" ]; then
	docker compose build --no-cache
fi
docker compose up -d --build

# Wait for services to be healthy
echo -e "${YELLOW}Waiting for services to be healthy...${NC}"
timeout=300
elapsed=0
while [ $elapsed -lt $timeout ]; do
	# Check if all services are healthy
	healthy_count=$(docker compose ps 2>/dev/null | grep -c "(healthy)" || echo "0")
	total_services=6 # nfs-server, registry, postgres, qarax, qarax-node, qarax-node-2

	if [ "$healthy_count" -ge "$total_services" ]; then
		echo ""
		echo -e "${GREEN}All services are healthy!${NC}"
		break
	fi

	# Check for failed services
	if docker compose ps 2>/dev/null | grep -q "Exit"; then
		echo ""
		echo -e "${RED}A service has failed!${NC}"
		docker compose ps
		docker compose logs
		exit 1
	fi

	echo -n "."
	sleep 2
	elapsed=$((elapsed + 2))
done

if [ $elapsed -ge $timeout ]; then
	echo ""
	echo -e "${RED}Timeout waiting for services to be healthy${NC}"
	docker compose ps
	docker compose logs
	exit 1
fi

# Show service status
echo ""
echo -e "${YELLOW}Service status:${NC}"
docker compose ps
echo ""

bash setup_host.sh http://localhost:8000 qarax-node 50051 e2e-node-1
bash setup_host.sh http://localhost:8000 qarax-node-2 50051 e2e-node-2
echo ""

# Setup Python environment with uv
echo -e "${YELLOW}Installing test dependencies...${NC}"
uv sync --frozen

# Run the tests
export WEBHOOK_HOST="${WEBHOOK_HOST:-$(default_webhook_host)}"
echo -e "${YELLOW}Using webhook host:${NC} ${WEBHOOK_HOST}"
echo -e "${YELLOW}Running E2E tests...${NC}"
if uv run pytest -v "$@"; then
	echo ""
	echo -e "${GREEN}All tests passed!${NC}"
	exit 0
else
	echo ""
	echo -e "${RED}Tests failed!${NC}"
	echo ""
	echo -e "${YELLOW}Service logs:${NC}"
	docker compose logs --tail=50
	echo ""

	if [ -z "$KEEP" ]; then
		echo -e "${YELLOW}To keep services running for debugging:${NC}"
		echo -e "${GREEN}  KEEP=1 ./run_e2e_tests.sh${NC}"
		echo ""
	fi

	exit 1
fi
