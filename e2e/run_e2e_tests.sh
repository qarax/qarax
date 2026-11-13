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
#   REBUILD=1       - Rebuild the Docker images without cache
#   KEEP=1          - Keep services running after tests (for debugging)
#   SKIP_BUILD=1    - Use the existing release binaries (CI downloads them)
#   ENABLE_OTEL=1   - Build with `otel` features and point services at a host OTLP receiver
#
# A stack that was already up when this started (e.g. from `make run-local`,
# which shares the compose project) is left running afterwards, as with KEEP=1.

: "${CLOUD_HYPERVISOR_VERSION:=$(cat ../versions/cloud-hypervisor-version)}"
export CLOUD_HYPERVISOR_VERSION
: "${FIRECRACKER_VERSION:=$(cat ../versions/firecracker-version)}"
export FIRECRACKER_VERSION

# qarax CLI auth token, must match AUTH_TOKENS configured for the qarax
# service in docker-compose.yml (see e2e/helpers.py).
: "${QARAX_TOKEN:=${QARAX_TEST_TOKEN:-e2e-test-token}}"
export QARAX_TOKEN

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

is_truthy() {
	case "${1:-}" in
	1 | true | TRUE | yes | YES | on | ON) return 0 ;;
	*) return 1 ;;
	esac
}

needs_bootc_vm() {
	if is_truthy "${ENABLE_BOOTC_UPGRADE_E2E:-}"; then
		return 0
	fi

	for arg in "$@"; do
		case "$arg" in
		*test_node_upgrade.py*)
			return 0
			;;
		esac
	done

	return 1
}

docker_build_image() {
	local scope="$1"
	shift

	if is_truthy "${DOCKER_BUILDX:-}" || [ -n "${DOCKER_BUILD_CACHE_FROM:-}" ] || [ -n "${DOCKER_BUILD_CACHE_TO:-}" ]; then
		local cmd=(docker buildx build --load)
		if [ -n "${DOCKER_BUILD_CACHE_FROM:-}" ]; then
			cmd+=(--cache-from "${DOCKER_BUILD_CACHE_FROM},scope=${scope}")
		fi
		if [ -n "${DOCKER_BUILD_CACHE_TO:-}" ]; then
			cmd+=(--cache-to "${DOCKER_BUILD_CACHE_TO},scope=${scope}")
		fi
		"${cmd[@]}" "$@"
	else
		docker build "$@"
	fi
}

if curl -sf --max-time 3 http://localhost:8000/ >/dev/null 2>&1; then
	PREEXISTING_STACK=1
	echo -e "${YELLOW}A stack is already running on :8000; it will be left up afterwards.${NC}"
fi

cleanup() {
	if [ -n "$KEEP" ] || [ -n "${PREEXISTING_STACK:-}" ]; then
		echo ""
		echo -e "${GREEN}Services kept running${NC}"
		echo ""
		echo "Useful commands:"
		echo "  docker compose logs -f           # Follow logs"
		echo "  docker compose logs qarax-node   # qarax-node logs"
		echo "  docker compose exec qarax-node sh  # Shell into qarax-node"
		echo "  docker compose --profile '*' down -v   # Stop and remove everything"
		echo ""
		echo "Test endpoint:"
		echo "  curl http://localhost:8000/vms"
		echo ""
	else
		echo -e "${YELLOW}Cleaning up...${NC}"
		rm -f "${BOOTC_OVERLAY_DISK}"
		docker compose --profile '*' down -v
	fi
}

trap cleanup EXIT

# Build qarax binaries if needed (Linux musl binary for Docker)
MUSL_TARGET="x86_64-unknown-linux-musl"
BOOTC_OVERLAY_DISK="${PWD}/bootc-vm-overlay.qcow2"

CARGO_FEATURE_ARGS=()
if is_truthy "${ENABLE_OTEL:-}"; then
	OTEL_TEST_PORT="${OTEL_TEST_PORT:-14318}"
	OTEL_GATEWAY_HOST="${OTEL_GATEWAY_HOST:-$(default_webhook_host)}"
	export OTEL_ENABLED=true
	export OTEL_EXPORTER_OTLP_ENDPOINT="${OTEL_EXPORTER_OTLP_ENDPOINT:-http://${OTEL_GATEWAY_HOST}:${OTEL_TEST_PORT}}"
	export OTEL_BSP_SCHEDULE_DELAY="${OTEL_BSP_SCHEDULE_DELAY:-500}"
	export OTEL_METRIC_EXPORT_INTERVAL="${OTEL_METRIC_EXPORT_INTERVAL:-1000}"
	CARGO_FEATURE_ARGS=(--features "qarax/otel,qarax-node/otel")
	echo -e "${YELLOW}Telemetry e2e enabled:${NC} ${OTEL_EXPORTER_OTLP_ENDPOINT}"
fi

# Build on every run: cargo is a fast no-op when nothing changed, and an
# existence-only check would bake stale binaries into the images.
if [ -z "$SKIP_BUILD" ]; then
	BUILD_ARGS=(--release -p qarax -p qarax-node -p qarax-init -p cli "${CARGO_FEATURE_ARGS[@]}")
	echo -e "${YELLOW}Building qarax-server, qarax-node, qarax-init and the qarax CLI...${NC}"
	cd ..
	if [ "$(uname -s)" = "Darwin" ]; then
		if ! command -v cross &>/dev/null; then
			echo -e "${RED}Cross-compilation from macOS requires 'cross'. Install with: cargo install cross${NC}"
			exit 1
		fi
		cross build --target "${MUSL_TARGET}" "${BUILD_ARGS[@]}"
	elif [ -n "${SUDO_USER:-}" ]; then
		# Build as the invoking user so target/ stays user-owned.
		sudo -u "$SUDO_USER" cargo build "${BUILD_ARGS[@]}"
	else
		cargo build "${BUILD_ARGS[@]}"
	fi
	cd e2e
elif is_truthy "${ENABLE_OTEL:-}"; then
	echo -e "${YELLOW}SKIP_BUILD=1 set; make sure existing binaries were built with ENABLE_OTEL=1${NC}"
fi

# Bootc VM disk (real bootc upgrade e2e tests)─────────
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
	docker compose up -d --wait registry

	# Build base bootc image
	echo -e "${YELLOW}Building bootc node image...${NC}"
	local node_hash
	node_hash=$(sha256sum "${NODE_BIN}" | cut -d' ' -f1)
	docker_build_image "e2e-bootc-node-base" \
		--build-arg "CACHE_BUST=${node_hash}" \
		-f Containerfile.bootc-node \
		-t localhost:5001/qarax-node-bootc:base \
		..
	docker push localhost:5001/qarax-node-bootc:base

	# Push thin versioned images for bootc switch version tracking
	echo -e "${YELLOW}Pushing versioned bootc node images...${NC}"
	DOCKER_BUILDKIT=0 docker build -t localhost:5001/qarax-node-test:0.1.0 - <<'VEOF'
FROM localhost:5001/qarax-node-bootc:base
RUN mkdir -p /etc/qarax-node && printf 'QARAX_NODE_VERSION=0.1.0\n' > /etc/qarax-node/version.env
VEOF
	docker push localhost:5001/qarax-node-test:0.1.0

	DOCKER_BUILDKIT=0 docker build -t localhost:5001/qarax-node-test:0.2.0-test - <<'VEOF'
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

BOOTC_VM_ENABLED=0
if needs_bootc_vm "$@"; then
	BOOTC_VM_ENABLED=1
	build_bootc_vm
fi

SERVICES=(postgres nfs-server registry qarax-node qarax-node-2 qarax)
if [ "$BOOTC_VM_ENABLED" -eq 1 ]; then
	SERVICES+=(bootc-vm)
fi

# Build and start services
echo -e "${YELLOW}Starting services...${NC}"
if [ -n "$REBUILD" ]; then
	docker compose build --no-cache "${SERVICES[@]}"
fi
# --wait returns once every service's healthcheck passes. Naming the services
# also enables their compose profiles (nfs-server, qarax-node-2, bootc-vm).
if ! docker compose up -d --build --wait --wait-timeout 600 "${SERVICES[@]}"; then
	echo -e "${RED}Services did not become healthy${NC}"
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
