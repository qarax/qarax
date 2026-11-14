#!/usr/bin/env bash
#
# Run the local qarax stack in Docker: the control plane, qarax-node (with KVM
# passthrough for real VMs), Postgres and an OCI registry. It is the compose
# stack the e2e tests use (e2e/docker-compose.yml, project "e2e").
#
# Usage:
#   ./hack/run-local.sh            # build and start the stack       (make run-local)
#   ./hack/run-local.sh --vm       # the node as a bootc microVM      (make run-local VM=1)
#   ./hack/run-local.sh --cleanup  # remove the stack and its volumes (make stop-local)
#   SKIP_BUILD=1 ./hack/run-local.sh   # use the existing release binaries
#   REBUILD=1 ./hack/run-local.sh      # rebuild the Docker images without cache
#
# Requires Docker with Compose, /dev/kvm and a Rust toolchain. Afterwards the
# API is on http://localhost:8000 (token: $QARAX_TOKEN, default e2e-test-token),
# host "local-node" is UP and the shared OverlayBD pool "overlaybd-pool" exists,
# so the demos under demos/ run as is.
#
# --vm runs the node the way a real host does instead: the appliance image
# installed with bootc, booted as a Cloud Hypervisor microVM (compose service
# bootc-vm, the same node the e2e upgrade tests deploy to), registered as
# "local-node-vm". Each run boots it fresh from a disk rebuilt when the image
# changes. It needs qemu-img and nested virtualization; it runs OCI-image and
# disk-image VMs (the e2e test initramfs isn't on it).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Colors, die, QARAX_TOKEN, version exports and the CLI helpers.
source "${REPO_ROOT}/demos/lib.sh"

E2E_DIR="${REPO_ROOT}/e2e"
COMPOSE=(docker compose -f "${E2E_DIR}/docker-compose.yml")
RELEASE_DIR="${REPO_ROOT}/target/${MUSL_TARGET}/release"

usage() {
	sed -n '3,17p' "$0" | sed 's/^# \{0,1\}//'
}

# ── Cleanup ──────────────────────────────────────────────────────────────────
#
# The e2e runner shares this compose project and may have started nfs-server.
# Its nfsd kernel threads run on the HOST and outlive the container's init, so
# a plain `docker compose down` can wedge on it; stop nfsd from inside first.

lookup_nfs_container_id() {
	docker ps -a --filter "label=com.docker.compose.project=e2e" \
		--filter "label=com.docker.compose.service=nfs-server" --format "{{.ID}}" | head -1
}

container_status() {
	docker inspect --format "{{.State.Status}}" "$1" 2>/dev/null || true
}

graceful_stop_nfs_container() {
	local cid
	cid=$(lookup_nfs_container_id)
	[[ -n "$cid" && "$(container_status "$cid")" == "running" ]] || return 0

	echo -e "${YELLOW}Stopping NFS container cleanly...${NC}"
	# Best effort: whatever this fails to stop is force-removed below.
	docker exec "$cid" /bin/sh -c '
		exportfs -au; exportfs -f; rpc.nfsd 0; umount /proc/fs/nfsd; kill -TERM 1
	' >/dev/null 2>&1 || true
	wait_until 10 container_not_running "$cid" || true
}

container_not_running() {
	[[ "$(container_status "$1")" != "running" ]]
}

# Fallback for a wedged NFS container: kill the host-side nfsd threads that
# hold its cgroup open, then remove it.
force_remove_nfs_container() {
	local cid
	cid=$(lookup_nfs_container_id)
	[[ -n "$cid" ]] || return 0

	echo -e "${YELLOW}Removing NFS container (killing host-side nfsd threads)...${NC}"
	pkill -9 -x nfsd || true
	pkill -9 -x nfsd4 || true
	local full_id procs
	full_id=$(docker inspect --format '{{.Id}}' "$cid")
	procs="/sys/fs/cgroup/system.slice/docker-${full_id}.scope/cgroup.procs"
	if [[ -f "$procs" ]]; then
		xargs -r kill -9 <"$procs" 2>/dev/null || true
	fi
	wait_until 15 remove_container "$cid" || die "could not remove NFS container ${cid}"
}

remove_container() {
	docker rm -f "$1" >/dev/null 2>&1 || true
	! docker inspect "$1" >/dev/null 2>&1
}

cleanup_stack() {
	echo -e "${YELLOW}Removing the local stack and its volumes...${NC}"
	graceful_stop_nfs_container
	(cd "$E2E_DIR" && docker compose --profile '*' down -v --remove-orphans) ||
		warn "docker compose down reported an error; removing leftovers directly"
	force_remove_nfs_container

	# Demos can add services and volumes to the project from compose files of
	# their own (demos/block-storage), which the down above doesn't know about.
	local label="label=com.docker.compose.project=e2e" leftovers
	leftovers=$(docker ps -aq --filter "$label")
	if [[ -n "$leftovers" ]]; then
		# shellcheck disable=SC2086 # one container ID per word
		docker rm -f $leftovers >/dev/null
	fi
	leftovers=$(docker volume ls -q --filter "$label")
	if [[ -n "$leftovers" ]]; then
		# shellcheck disable=SC2086 # one volume name per word
		docker volume rm $leftovers >/dev/null
	fi
	leftovers=$(docker network ls -q --filter "$label")
	if [[ -n "$leftovers" ]]; then
		# shellcheck disable=SC2086 # one network ID per word
		docker network rm $leftovers >/dev/null
	fi
	leftovers="$(docker ps -aq --filter "$label")$(docker network ls -q --filter "$label")$(docker volume ls -q --filter "$label")"
	[[ -z "$leftovers" ]] || die "cleanup incomplete; inspect: docker ps -a / network ls / volume ls --filter ${label}"
	# Boot images from the old run-local.sh --with-vm, still on disk in older checkouts.
	rm -rf "${E2E_DIR}/local-test-images"
	echo -e "${GREEN}Done.${NC}"
}

# ── Start ────────────────────────────────────────────────────────────────────

preflight() {
	command -v docker >/dev/null || die "Docker is required."
	[[ -e /dev/kvm ]] || die "/dev/kvm not found: qarax-node needs KVM (a Linux host with VT-x/AMD-V, or nested virtualization)."
	if [[ ! -e /dev/net/tun ]]; then
		warn "/dev/net/tun not found: VMs with network interfaces will fail to start. Create it, then recreate the stack:"
		warn "  sudo modprobe tun && sudo mkdir -p /dev/net && sudo mknod /dev/net/tun c 10 200 && sudo chmod 0666 /dev/net/tun"
	fi
	if [[ "$VM_MODE" -eq 1 ]]; then
		command -v qemu-img >/dev/null || die "--vm needs qemu-img to build the node's disk."
	fi
}

# Build the binaries the images package, plus the CLI the demos use. Cargo is
# a fast no-op when nothing changed.
build_binaries() {
	local packages=(-p qarax -p qarax-node -p qarax-init -p cli)
	if [[ -n "${SKIP_BUILD:-}" ]]; then
		local bin
		for bin in qarax-server qarax-node qarax-init qarax; do
			[[ -f "${RELEASE_DIR}/${bin}" ]] || die "SKIP_BUILD=1 but ${RELEASE_DIR}/${bin} is missing."
		done
		echo -e "${YELLOW}Using existing binaries (SKIP_BUILD=1)${NC}"
		return
	fi

	echo -e "${YELLOW}Building qarax, qarax-node, qarax-init and the CLI (release, musl)...${NC}"
	if [[ "$(uname -s)" == "Darwin" ]]; then
		# The system linker can't cross-link musl from macOS.
		command -v cross >/dev/null || die "Cross-compiling from macOS needs 'cross': cargo install cross"
		(cd "$REPO_ROOT" && cross build --target "${MUSL_TARGET}" --release "${packages[@]}")
	elif [[ -n "${SUDO_USER:-}" ]]; then
		# Build as the invoking user so target/ stays user-owned.
		(cd "$REPO_ROOT" && sudo -u "$SUDO_USER" cargo build --release "${packages[@]}")
	else
		(cd "$REPO_ROOT" && cargo build --release "${packages[@]}")
	fi
}

start_services() {
	local services=("$@")
	if [[ -n "${REBUILD:-}" ]]; then
		"${COMPOSE[@]}" build --no-cache "${services[@]}"
	fi
	echo -e "${YELLOW}Starting ${services[*]}...${NC}"
	# --wait returns once every service's healthcheck passes; Compose only
	# recreates containers whose image or configuration changed.
	if ! "${COMPOSE[@]}" up -d --build --wait --wait-timeout 300 "${services[@]}"; then
		"${COMPOSE[@]}" ps
		"${COMPOSE[@]}" logs --tail=80
		die "the stack did not become healthy"
	fi
}

# Create the shared OverlayBD pool the demos import images into, and attach
# every UP host. Attaching is idempotent, so re-runs pick up new hosts.
ensure_overlaybd_pool() {
	local registry_url="$1" existing_url
	existing_url=$(qarax storage-pool list -o json |
		jq -r '.[] | select(.name == "overlaybd-pool") | .config.url')
	if [[ -z "$existing_url" ]]; then
		qarax storage-pool create --name overlaybd-pool --pool-type overlaybd --url "$registry_url" >/dev/null
	elif [[ "$existing_url" != "$registry_url" ]]; then
		warn "overlaybd-pool points at ${existing_url}, not ${registry_url}."
		warn "It was created for the other node mode; run 'make stop-local' to start fresh."
	fi
	qarax storage-pool attach-host overlaybd-pool --all >/dev/null
}

# Each mode has one node: stop the other mode's node (SERVICE) if an earlier
# run left it up, and mark its host down so nothing gets scheduled there.
stop_other_node() {
	local service="$1" id
	[[ -n "$("${COMPOSE[@]}" --profile '*' ps -q "$service" 2>/dev/null)" ]] || return 0
	"${COMPOSE[@]}" --profile '*' stop "$service"
	id=$(qarax host list -o json | jq -r --arg a "$service" '.[] | select(.address == $a and .status == "up") | .id')
	[[ -n "$id" ]] || return 0
	api_curl -fsS -X PATCH "${SERVER}/hosts/${id}" -H "Content-Type: application/json" \
		-d '{"status":"down"}' >/dev/null
	warn "Stopped ${service} and marked its host DOWN; this mode's node is ${NODE_SERVICE}."
}

print_summary() {
	cat <<EOF

$(echo -e "${GREEN}Qarax is running.${NC}")

  API:          ${SERVER}/
  Swagger UI:   ${SERVER}/swagger-ui
  Token:        export QARAX_TOKEN=${QARAX_TOKEN}
  CLI:          ${QARAX_BIN} (or cargo run -p cli --)

Try it:
  qarax host list
  qarax vm create --name alpine --image-ref public.ecr.aws/docker/library/alpine:latest
  qarax vm start alpine
  ./demos/oci/run.sh            # more in demos/README.md

Logs:   docker compose -f e2e/docker-compose.yml logs -f qarax ${NODE_SERVICE}
Stop:   make stop-local
EOF
	if [[ "$VM_MODE" -eq 1 ]]; then
		cat <<EOF
Node:   a bootc microVM (serial console in the logs above); shell, password testpassword:
        docker compose -f e2e/docker-compose.yml exec bootc-vm ssh root@10.0.2.15
EOF
	fi
}

VM_MODE=0
for arg in "$@"; do
	case "$arg" in
	--vm) VM_MODE=1 ;;
	--cleanup)
		cleanup_stack
		exit 0
		;;
	--with-vm) die "--with-vm was removed: ./demos/boot-source/run.sh or ./demos/cloud-image/run.sh boot an example VM on the running stack." ;;
	-h | --help)
		usage
		exit 0
		;;
	*) die "unknown option: ${arg} (see --help)" ;;
	esac
done

preflight
build_binaries
QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found after the build"

if [[ "$VM_MODE" -eq 1 ]]; then
	NODE_SERVICE=bootc-vm
	start_services registry postgres qarax
	echo -e "${YELLOW}Building the bootc node image and disk...${NC}"
	bash "${E2E_DIR}/build-bootc-node.sh"
	# The build gave the disk a fresh overlay; boot a node VM on it.
	COMPOSE+=(--profile bootc)
	"${COMPOSE[@]}" up -d --build --force-recreate --wait --wait-timeout 300 bootc-vm
	stop_other_node qarax-node
	bash "${E2E_DIR}/setup_host.sh" "$SERVER" bootc-vm 50051 local-node-vm
	# The node resolves "registry" to its gateway, which proxies the
	# registry, so the pool URL is the same as in container mode.
	ensure_overlaybd_pool "http://registry:5000"
else
	NODE_SERVICE=qarax-node
	services=(registry postgres qarax-node qarax)
	# A two-host demo may have started qarax-node-2 (profile multi-node); keep
	# it on the same images as the first node.
	if [[ -n "$("${COMPOSE[@]}" --profile multi-node ps -q qarax-node-2 2>/dev/null)" ]]; then
		COMPOSE+=(--profile multi-node)
		services+=(qarax-node-2)
	fi
	start_services "${services[@]}"
	stop_other_node bootc-vm
	bash "${E2E_DIR}/setup_host.sh" "$SERVER" qarax-node 50051 local-node
	if [[ " ${services[*]} " == *" qarax-node-2 "* ]]; then
		# Init re-creates the networks and pools a recreated container lost.
		bash "${E2E_DIR}/setup_host.sh" "$SERVER" qarax-node-2 50051 local-node-2
	fi
	ensure_overlaybd_pool "http://registry:5000"
fi

print_summary
