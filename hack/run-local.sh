#!/usr/bin/env bash
#
# Run qarax control plane + qarax-node + PostgreSQL in Docker for local testing.
# Uses the same stack as E2E: Docker Compose with KVM passthrough for real VMs.
#
# Requirements:
#   - Docker (with Compose)
#   - KVM: /dev/kvm must be available (native Linux with KVM or nested virt)
#   - Rust toolchain (to build qarax-node binary for the node container)
#
# Usage:
#   ./hack/run-local.sh            # Build and start the stack
#   ./hack/run-local.sh --vm         # Run qarax-node in a libvirt VM instead of a container (alias: --with-vm)
#   ./hack/run-local.sh --cleanup  # Stop and remove stack + volumes
#   REBUILD=1 ./hack/run-local.sh  # Rebuild Docker images from scratch
#   SKIP_BUILD=1 ./hack/run-local.sh # Use existing qarax-node binary
#
# After start:
#   API:        http://localhost:8000
#   Swagger UI: http://localhost:8000/swagger-ui
#
# Typical workflow (OverlayBD disk-first):
#   1. Import OCI image into pool:
#      qarax storage-pool import --pool <pool> --image-ref <ref> --name <name>
#   2. Create VM:
#      qarax vm create --name my-vm --vcpus 1 --memory 268435456
#   3. Attach disk to VM:
#      qarax vm attach-disk my-vm --object <name>
#   4. Start VM:
#      qarax vm start <VM_ID>
#

set -e

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

CH_VERSION_FILE="${REPO_ROOT}/versions/cloud-hypervisor-version"
export CLOUD_HYPERVISOR_VERSION="${CLOUD_HYPERVISOR_VERSION:-$(tr -d '\n' <"$CH_VERSION_FILE")}"

COMPOSE_ARGS=()
E2E_COMPOSE_ARGS=(-f "${REPO_ROOT}/e2e/docker-compose.yml")

compose_cmd() {
	docker compose "${COMPOSE_ARGS[@]}" "$@"
}

e2e_compose_cmd() {
	docker compose "${E2E_COMPOSE_ARGS[@]}" "$@"
}

kill_processes_by_exact_name() {
	local name="$1"
	local pids

	pids=$(pgrep -x "$name" 2>/dev/null || true)
	[[ -z "$pids" ]] && return 0

	while IFS= read -r pid; do
		[[ -n "$pid" ]] || continue
		kill -9 "$pid" 2>/dev/null || true
	done <<<"$pids"
}

kill_container_scope_processes() {
	local cid="$1"
	local scope="/sys/fs/cgroup/system.slice/docker-${cid}.scope/cgroup.procs"
	[[ -f "$scope" ]] || return 0

	python3 - "$scope" <<'PY'
import os
import signal
import sys

scope = sys.argv[1]

with open(scope, encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue

        pid = int(line)
        if pid in {os.getpid(), os.getppid()}:
            continue

        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except PermissionError:
            pass
PY
}

lookup_nfs_container_id() {
	docker ps -a --filter "name=e2e-nfs-server-1" --format "{{.ID}}" 2>/dev/null | head -1
}

graceful_stop_nfs_container() {
	local cid
	cid=$(lookup_nfs_container_id)
	[[ -z "$cid" ]] && return 0

	local status
	status=$(docker inspect --format "{{.State.Status}}" "$cid" 2>/dev/null || echo "")
	[[ "$status" != "running" ]] && return 0

	echo -e "${YELLOW}Stopping NFS container cleanly...${NC}"
	docker exec "$cid" /bin/sh -c '
		exportfs -au 2>/dev/null || true
		exportfs -f 2>/dev/null || true
		rpc.nfsd 0 2>/dev/null || true
		umount /proc/fs/nfsd 2>/dev/null || true
		kill -TERM 1
	' >/dev/null 2>&1 || true

	local attempt
	for attempt in $(seq 1 10); do
		status=$(docker inspect --format "{{.State.Status}}" "$cid" 2>/dev/null || echo "")
		[[ -z "$status" ]] && return 0
		[[ "$status" != "running" ]] && return 0
		sleep 1
	done
}

# Fallback for previously wedged containers that did not shut down nfsd cleanly.
force_remove_nfs_container() {
	local cid
	cid=$(lookup_nfs_container_id)
	[[ -z "$cid" ]] && return 0

	local status
	status=$(docker inspect --format "{{.State.Status}}" "$cid" 2>/dev/null || echo "")
	[[ -z "$status" ]] && return 0

	echo -e "${YELLOW}Removing NFS container (killing host-side nfsd kernel threads)...${NC}"

	# nfsd spawns kernel threads on the HOST that survive SIGKILL sent to the
	# container's init process. These threads hold the container's cgroup open,
	# making docker stop/rm impossible. Kill them directly on the host first.
	kill_processes_by_exact_name nfsd
	kill_processes_by_exact_name nfsd4
	kill_container_scope_processes "$cid"

	local attempt
	local status
	for attempt in $(seq 1 15); do
		status=$(docker inspect --format "{{.State.Status}}" "$cid" 2>/dev/null || echo "")
		[[ -z "$status" ]] && return 0

		if [[ "$status" == "running" ]] || [[ "$status" == "restarting" ]]; then
			docker kill --signal=SIGKILL "$cid" 2>/dev/null || true
		fi

		docker rm -f "$cid" 2>/dev/null || true

		if ! docker inspect "$cid" >/dev/null 2>&1; then
			return 0
		fi

		sleep 1
	done
}

force_remove_compose_containers() {
	local ids
	ids=$(list_compose_container_ids)
	[[ -z "$ids" ]] && return 0

	echo -e "${YELLOW}Force-removing lingering Compose containers...${NC}"

	local cid
	local status
	while IFS= read -r cid; do
		[[ -n "$cid" ]] || continue
		status=$(docker inspect --format "{{.State.Status}}" "$cid" 2>/dev/null || echo "")
		[[ -z "$status" ]] && continue

		if [[ "$status" == "running" ]] || [[ "$status" == "restarting" ]]; then
			docker kill --signal=SIGKILL "$cid" 2>/dev/null || true
		fi

		docker rm -f "$cid" 2>/dev/null || true
	done <<<"$ids"
}

list_compose_containers() {
	docker ps -a \
		--filter "label=com.docker.compose.project=e2e" \
		--format "{{.Names}} {{.Status}}" 2>/dev/null || true
}

list_compose_container_ids() {
	docker ps -a \
		--filter "label=com.docker.compose.project=e2e" \
		--format "{{.ID}}" 2>/dev/null || true
}

list_compose_networks() {
	docker network ls \
		--filter "label=com.docker.compose.project=e2e" \
		--format "{{.Name}}" 2>/dev/null || true
}

list_compose_volumes() {
	docker volume ls \
		--filter "label=com.docker.compose.project=e2e" \
		--format "{{.Name}}" 2>/dev/null || true
}

purge_local_host_records() {
	local postgres_container
	postgres_container="$(
		docker ps \
			--filter "label=com.docker.compose.project=e2e" \
			--filter "label=com.docker.compose.service=postgres" \
			--format "{{.Names}}" 2>/dev/null | head -1
	)"
	[[ -z "$postgres_container" ]] && return 0

	echo -e "${YELLOW}Clearing local host records from Postgres...${NC}"
	docker exec "$postgres_container" psql -U qarax -d qarax -v ON_ERROR_STOP=1 -c "
BEGIN;
UPDATE vms SET host_id = NULL;
DELETE FROM hosts;
COMMIT;
" >/dev/null
}

lookup_host_id_by_address() {
	local api_url="$1"
	local address="$2"

	curl -fsS "${api_url}/hosts" | python3 -c "
import json, sys
address = sys.argv[1]
for host in json.load(sys.stdin):
    if host.get('address') == address:
        print(host['id'])
        break
" "$address"
}

mark_host_down_by_address() {
	local api_url="$1"
	local address="$2"
	local host_id

	host_id="$(lookup_host_id_by_address "$api_url" "$address" || true)"
	[[ -z "$host_id" ]] && return 0

	curl -fsS -X PATCH "${api_url}/hosts/${host_id}" \
		-H "Content-Type: application/json" \
		-d '{"status":"down"}' >/dev/null
	echo -e "${YELLOW}Marked stale host ${address} DOWN (id: ${host_id}).${NC}"
}

cleanup_stack() {
	local down_failed=0

	purge_local_host_records
	graceful_stop_nfs_container

	if ! docker compose down -v; then
		down_failed=1
	fi

	force_remove_nfs_container
	force_remove_compose_containers

	local remaining
	remaining=$(list_compose_containers)
	if [[ -n "$remaining" ]]; then
		echo -e "${RED}Cleanup incomplete. Remaining Compose containers:${NC}" >&2
		echo "$remaining" >&2
		exit 1
	fi

	local remaining_networks
	remaining_networks=$(list_compose_networks)
	if [[ -n "$remaining_networks" ]]; then
		echo -e "${RED}Cleanup incomplete. Remaining Compose networks:${NC}" >&2
		echo "$remaining_networks" >&2
		exit 1
	fi

	local remaining_volumes
	remaining_volumes=$(list_compose_volumes)
	if [[ -n "$remaining_volumes" ]]; then
		echo -e "${RED}Cleanup incomplete. Remaining Compose volumes:${NC}" >&2
		echo "$remaining_volumes" >&2
		exit 1
	fi

	if [[ "$down_failed" -eq 1 ]]; then
		echo -e "${YELLOW}Compose reported an NFS shutdown error, but the stack is now removed.${NC}"
	fi
}

show_compose_diagnostics() {
	compose_cmd ps
	compose_cmd logs --tail=80
}

print_api_endpoints() {
	cat <<EOF
Endpoints:
  API (root):   http://localhost:8000/
  Swagger UI:   http://localhost:8000/swagger-ui
  OpenAPI JSON: http://localhost:8000/api-docs/openapi.json

EOF
}

print_overlaybd_workflow() {
	local pool_name="${1:-<POOL_NAME_OR_ID>}"

	cat <<EOF
Explicit disk workflow (OverlayBD):
  # 1. Import an OCI image into the storage pool (async, polls to completion):
  cargo run -p cli storage-pool import --pool ${pool_name} \\
    --image-ref public.ecr.aws/docker/library/alpine:latest \\
    --name alpine-obd

  # 2. Create a VM:
  cargo run -p cli vm create --name my-vm --vcpus 1 --memory 268435456

  # 3. Link the imported storage object as the boot disk:
  cargo run -p cli vm attach-disk my-vm --object alpine-obd

  # 4. Start the VM (provisions on node + boots):
  cargo run -p cli vm start my-vm

EOF
}

print_command_section() {
	local title="$1"
	shift

	echo "${title}"
	for command in "$@"; do
		echo "  ${command}"
	done
	echo ""
}

set_compose_service_lists() {
	if [[ "$VM_MODE" -eq 1 ]]; then
		UP_SERVICES=(registry postgres qarax)
		BUILD_SERVICES=(qarax)
		FORCE_RECREATE_SERVICES=(qarax)
		TOTAL_SERVICES=3
	else
		UP_SERVICES=(nfs-server registry postgres qarax-node qarax)
		BUILD_SERVICES=(nfs-server qarax-node qarax)
		FORCE_RECREATE_SERVICES=(qarax qarax-node)
		TOTAL_SERVICES=4
	fi
}

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

WITH_VM=0
VM_MODE=0
for arg in "$@"; do
	case $arg in
	--vm)
		VM_MODE=1
		shift
		;;
	--with-vm)
		WITH_VM=1
		shift
		;;
	--cleanup)
		echo "===== Qarax local cleanup ====="
		cd "${REPO_ROOT}/e2e"
		echo -e "${YELLOW}Stopping and removing stack (postgres, qarax, qarax-node) and volumes...${NC}"
		cleanup_stack
		# Clean up local test images if they exist
		if [[ -d "${REPO_ROOT}/e2e/local-test-images" ]]; then
			echo -e "${YELLOW}Removing local test kernel/initramfs/rootfs...${NC}"
			rm -rf "${REPO_ROOT}/e2e/local-test-images"
		fi
		echo -e "${GREEN}Done.${NC}"
		exit 0
		;;
	esac
done

echo "===== Qarax local run (Docker stack) ====="

# Preflight
if ! command -v docker &>/dev/null; then
	echo -e "${RED}Docker is required. Install Docker and try again.${NC}"
	exit 1
fi

if [[ ! -e /dev/kvm ]]; then
	echo -e "${RED}/dev/kvm not found.${NC}"
	echo "qarax-node needs KVM to run VMs. Options:"
	echo "  - Run on a Linux host with KVM (e.g. Intel VT-x / AMD-V)"
	echo "  - Use a VM with nested virtualization and /dev/kvm exposed"
	exit 1
fi

if [[ ! -e /dev/net/tun ]]; then
	echo -e "${YELLOW}Warning: /dev/net/tun not found on host.${NC}"
	echo "VMs with network interfaces will fail to start (virtio-net needs it)."
	echo "Create it on the host, then recreate the stack:"
	echo "  sudo modprobe tun && sudo mkdir -p /dev/net && sudo mknod /dev/net/tun c 10 200 && sudo chmod 0666 /dev/net/tun"
	echo "  ./hack/run-local.sh --cleanup && ./hack/run-local.sh"
	echo ""
fi

# Build the release binaries that local Docker/VM flows package and run.
# We invoke the build step on every run so startup always uses fresh artifacts;
# Cargo/Cross will be a fast no-op when nothing changed.
# Use cross on macOS (system linker doesn't support musl cross-compile); cargo on Linux.
MUSL_TARGET="x86_64-unknown-linux-musl"
NODE_BINARY="${REPO_ROOT}/target/${MUSL_TARGET}/release/qarax-node"
QARAX_BINARY="${REPO_ROOT}/target/${MUSL_TARGET}/release/qarax-server"
INIT_BINARY="${REPO_ROOT}/target/${MUSL_TARGET}/release/qarax-init"
if [[ -z "${SKIP_BUILD}" ]]; then
	echo -e "${YELLOW}Building qarax, qarax-node, and qarax-init (release, musl)...${NC}"
	if [[ "$(uname -s)" == "Darwin" ]]; then
		if ! command -v cross &>/dev/null; then
			echo -e "${RED}Cross-compilation from macOS requires 'cross'. Install with: cargo install cross${NC}"
			exit 1
		fi
		cross build --target "${MUSL_TARGET}" --release -p qarax -p qarax-node -p qarax-init
	else
		# If running under sudo, build as the original user so target/ stays user-owned.
		if [[ -n "${SUDO_USER:-}" ]]; then
			sudo -u "$SUDO_USER" cargo build --release -p qarax -p qarax-node -p qarax-init
		else
			cargo build --release -p qarax -p qarax-node -p qarax-init
		fi
	fi
else
	if [[ ! -f "${NODE_BINARY}" ]] || [[ ! -f "${QARAX_BINARY}" ]] || [[ ! -f "${INIT_BINARY}" ]]; then
		echo -e "${RED}SKIP_BUILD=1 but binaries not found. Build first or remove SKIP_BUILD.${NC}"
		exit 1
	fi
	echo -e "${YELLOW}Skipping build (SKIP_BUILD=1)${NC}"
fi

# Build rootfs before starting the stack (if --with-vm)
BOOT_IMAGES_DIR=""
export PRODUCTION_ROOTFS=""
if [[ $WITH_VM -eq 1 ]]; then
	BOOT_IMAGES_DIR="${REPO_ROOT}/e2e/local-test-images"
	ROOTFS_IMG="${BOOT_IMAGES_DIR}/rootfs.img"

	if [[ -f "$ROOTFS_IMG" ]]; then
		echo -e "${GREEN}Using existing rootfs: $ROOTFS_IMG${NC}"
	else
		mkdir -p "$BOOT_IMAGES_DIR"
		echo -e "${YELLOW}Building Alpine Linux rootfs with SSH (this takes a few minutes)...${NC}"

		cat >/tmp/build-rootfs-$$.sh <<'ROOTFS_SCRIPT'
#!/bin/sh
set -e
apk add --no-cache e2fsprogs wget util-linux >/dev/null 2>&1
# Ensure loop devices exist (may be absent inside Docker even with --privileged)
if [ ! -e /dev/loop-control ]; then
    mknod /dev/loop-control c 10 237
fi
for i in $(seq 0 7); do
    [ ! -e "/dev/loop$i" ] && mknod "/dev/loop$i" b 7 "$i"
done
echo "Creating 1GB rootfs image..."
dd if=/dev/zero of=/output/rootfs.img bs=1M count=1024
echo "Formatting with ext4..."
mkfs.ext4 -F /output/rootfs.img
echo "Mounting rootfs..."
mkdir -p /mnt/rootfs
LOOP=$(losetup --find --show /output/rootfs.img)
mount "$LOOP" /mnt/rootfs
echo "Installing Alpine Linux..."
ALPINE_VERSION="3.19"
wget -q -O /tmp/alpine.tar.gz \
  "https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION}/releases/x86_64/alpine-minirootfs-${ALPINE_VERSION}.1-x86_64.tar.gz"
tar xzf /tmp/alpine.tar.gz -C /mnt/rootfs
rm /tmp/alpine.tar.gz
echo "nameserver 8.8.8.8" > /mnt/rootfs/etc/resolv.conf
cat > /mnt/rootfs/etc/network/interfaces << 'NET_EOF'
auto lo
iface lo inet loopback

auto eth0
iface eth0 inet static
    address 192.168.100.2
    netmask 255.255.255.0
    gateway 192.168.100.1
NET_EOF
mkdir -p /mnt/rootfs/etc/ssh
cat > /mnt/rootfs/etc/ssh/sshd_config << 'SSH_EOF'
PermitRootLogin yes
PasswordAuthentication yes
PrintMotd no
Subsystem sftp /usr/lib/ssh/sftp-server
SSH_EOF
echo "root:qarax" | chroot /mnt/rootfs /usr/sbin/chpasswd
chroot /mnt/rootfs /bin/sh << 'CHROOT_EOF'
apk add --no-cache openssh openrc util-linux
rc-update add sshd default
rc-update add networking boot
rc-update add devfs boot
rc-update add procfs boot
rc-update add sysfs boot
CHROOT_EOF
cat > /mnt/rootfs/etc/fstab << 'FSTAB_EOF'
/dev/vda    /    ext4    defaults    0 1
FSTAB_EOF
echo "Rootfs setup complete"
umount /mnt/rootfs
losetup -d "$LOOP"
chmod 666 /output/rootfs.img
ls -lh /output/rootfs.img
ROOTFS_SCRIPT

		chmod +x /tmp/build-rootfs-$$.sh
		LOOP_DEVICES=()
		[[ -e /dev/loop-control ]] && LOOP_DEVICES+=(--device /dev/loop-control)
		for ld in /dev/loop[0-9]*; do
			[[ -e "$ld" ]] && LOOP_DEVICES+=("--device=$ld")
		done
		docker run --rm --privileged \
			"${LOOP_DEVICES[@]}" \
			-v "${BOOT_IMAGES_DIR}:/output" \
			-v "/tmp/build-rootfs-$$.sh:/build-rootfs.sh:ro" \
			alpine:3.19 sh /build-rootfs.sh
		rm -f /tmp/build-rootfs-$$.sh
		echo -e "${GREEN}Rootfs built: $ROOTFS_IMG${NC}"
	fi

	export PRODUCTION_IMAGES_DIR="$BOOT_IMAGES_DIR"
	export PRODUCTION_ROOTFS="/var/lib/qarax/production-images/rootfs.img"
fi

# Start stack (postgres + qarax [+ qarax-node unless --vm])
echo -e "${YELLOW}Starting Docker stack...${NC}"
cd "${REPO_ROOT}/e2e"

COMPOSE_ARGS=(-f docker-compose.yml)
if [[ $VM_MODE -eq 1 ]]; then
	COMPOSE_ARGS+=(-f docker-compose.vm-mode.yml)
fi
set_compose_service_lists

if [[ -n "${REBUILD}" ]]; then
	compose_cmd build --no-cache "${BUILD_SERVICES[@]}"
fi
force_remove_nfs_container
compose_cmd up -d --build "${UP_SERVICES[@]}"
compose_cmd up -d --force-recreate "${FORCE_RECREATE_SERVICES[@]}"

# Wait for services to be healthy
echo -e "${YELLOW}Waiting for services to be healthy...${NC}"
timeout=90
elapsed=0
while [[ $elapsed -lt $timeout ]]; do
	compose_ps=$(compose_cmd ps 2>/dev/null || true)
	healthy_count=$(printf '%s\n' "$compose_ps" | grep -c '(healthy)' || echo "0")

	if [[ "$healthy_count" -ge "$TOTAL_SERVICES" ]]; then
		echo ""
		echo -e "${GREEN}All services are healthy.${NC}"
		break
	fi

	if printf '%s\n' "$compose_ps" | grep -q "Exit"; then
		echo ""
		echo -e "${RED}A service has failed.${NC}"
		show_compose_diagnostics
		exit 1
	fi

	echo -n "."
	sleep 2
	elapsed=$((elapsed + 2))
done

if [[ $elapsed -ge $timeout ]]; then
	echo ""
	echo -e "${RED}Timeout waiting for services.${NC}"
	show_compose_diagnostics
	exit 1
fi

# --vm mode: launch a libvirt VM as the qarax-node host
if [[ $VM_MODE -eq 1 ]]; then
	echo ""
	echo -e "${YELLOW}Launching libvirt VM as qarax-node host...${NC}"
	LOCAL_API_URL="${API_URL:-http://localhost:8000}"
	API_URL="${LOCAL_API_URL}" \
		bash "${REPO_ROOT}/hack/test-host-deploy-libvirt.sh" --keep-vm
	mark_host_down_by_address "${LOCAL_API_URL}" "qarax-node"

	echo ""
	echo -e "${YELLOW}Creating overlaybd storage pool...${NC}"
	# The VM reaches the Docker-hosted registry via the libvirt bridge IP, not
	# the Compose DNS name "registry" which is only resolvable inside Docker.
	LIBVIRT_BRIDGE_IP="${LIBVIRT_BRIDGE_IP:-192.168.122.1}"
	setup_output=$(python3 "${REPO_ROOT}/hack/setup_vm.py" \
		--skip-vm \
		--skip-host \
		--kernel-path /dev/null \
		--overlaybd-registry-url "http://${LIBVIRT_BRIDGE_IP}:5001")
	eval "$setup_output"
	echo -e "${GREEN}Storage pool created: ${OVERLAYBD_POOL_ID}${NC}"

	echo ""
	echo -e "${GREEN}Qarax stack ready (node running in libvirt VM).${NC}"
	echo ""
	print_api_endpoints
	print_overlaybd_workflow "overlaybd-pool"
	print_command_section "Other useful commands:" \
		"cargo run -p cli vm list" \
		"cargo run -p cli vm get <VM_ID>" \
		"cargo run -p cli vm stop <VM_ID>" \
		"cargo run -p cli storage-pool list" \
		"cargo run -p cli job get <JOB_ID>" \
		"cargo run -p cli --json vm list              # raw JSON output"
	print_command_section "Docker stack commands:" \
		"docker compose -f e2e/docker-compose.yml logs -f qarax" \
		"docker compose -f e2e/docker-compose.yml logs -f qarax-node"
	echo "Cleanup: make stop-local (Docker stack) + virsh destroy ${VM_NAME:-qarax-deploy-test}"
	echo ""
	exit 0
fi

# Create and start a VM if --with-vm flag is set
if [[ $WITH_VM -eq 1 ]]; then
	echo ""
	echo -e "${YELLOW}Creating example VM with boot source...${NC}"

	# Build initramfs: loads virtio_net modules then switch_roots into Alpine on /dev/vda
	# (virtio_blk is built-in so /dev/vda is available at boot without modules)
	INITRAMFS_GZ="${REPO_ROOT}/e2e/local-test-images/boot-initramfs.gz"
	if [[ ! -f "$INITRAMFS_GZ" ]]; then
		echo -e "${YELLOW}Building boot initramfs...${NC}"
		KERNEL_VERSION=$(e2e_compose_cmd exec -T qarax-node ls /lib/modules/ | head -1 | tr -d '\r')
		MODULE_DIR="/tmp/qarax-mods-$$"
		mkdir -p "$MODULE_DIR"

		# Extract network modules from qarax-node
		cat >/tmp/get-mods-$$.sh <<GETMODS
#!/bin/sh
set -e
mkdir -p /tmp/mods
KDIR="/lib/modules/${KERNEL_VERSION}/kernel"
for name in failover net_failover virtio_net; do
  f=\$(find "\$KDIR" -name "\${name}.ko" -o -name "\${name}.ko.xz" 2>/dev/null | head -1)
  [ -z "\$f" ] && echo "MISSING: \$name" && continue
  cp "\$f" /tmp/mods/
  case "\$f" in *.xz) cd /tmp/mods && unxz "\$(basename \$f)" && cd -;; esac
  echo "OK: \$name"
done
ls /tmp/mods/
GETMODS
		chmod +x /tmp/get-mods-$$.sh
		docker cp /tmp/get-mods-$$.sh e2e-qarax-node-1:/tmp/get-mods.sh
		e2e_compose_cmd exec -T qarax-node /tmp/get-mods.sh
		docker cp e2e-qarax-node-1:/tmp/mods/. "$MODULE_DIR/"

		# Build the initramfs inside the qarax-node container (uses Fedora busybox)
		docker cp "$MODULE_DIR/." e2e-qarax-node-1:/tmp/bootmods/
		e2e_compose_cmd exec -T qarax-node sh -c '
      set -e
      mkdir -p /tmp/initrd/bin /tmp/initrd/dev /tmp/initrd/proc /tmp/initrd/sys /tmp/initrd/newroot /tmp/initrd/lib/modules
      cp /usr/sbin/busybox /tmp/initrd/bin/busybox
      for cmd in sh mount insmod sleep switch_root; do ln -sf busybox /tmp/initrd/bin/$cmd; done
      cp /tmp/bootmods/*.ko /tmp/initrd/lib/modules/ 2>/dev/null || true
      cat > /tmp/initrd/init << '"'"'INIT'"'"'
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
echo "Loading network modules..."
for m in failover.ko net_failover.ko virtio_net.ko; do
  [ -f "/lib/modules/$m" ] && insmod "/lib/modules/$m" 2>/dev/null && echo "  loaded $m" || true
done
echo "Waiting for /dev/vda..."
i=0
while [ ! -b /dev/vda ] && [ $i -lt 10 ]; do sleep 1; i=$((i+1)); done
if [ ! -b /dev/vda ]; then echo "ERROR: /dev/vda not found"; exec /bin/sh; fi
echo "Mounting rootfs on /dev/vda..."
mount /dev/vda /newroot
exec switch_root /newroot /sbin/init
INIT
      chmod +x /tmp/initrd/init
      cd /tmp/initrd && find . | cpio -o -H newc 2>/dev/null | gzip > /var/lib/qarax/production-images/boot-initramfs.gz
      echo "Initramfs built: $(ls -lh /var/lib/qarax/production-images/boot-initramfs.gz | awk '"'"'{print $5}'"'"')"
    '
		rm -f /tmp/get-mods-$$.sh
		rm -rf "$MODULE_DIR"
		echo -e "${GREEN}Boot initramfs built.${NC}"
	else
		echo -e "${GREEN}Using existing boot initramfs.${NC}"
	fi

	KERNEL_PATH="/var/lib/qarax/images/vmlinux"
	INITRAMFS_PATH="/var/lib/qarax/production-images/boot-initramfs.gz"
	CMDLINE="console=ttyS0"

	# Use Python script for all API interactions:
	# host registration, storage pool, transfers, boot source, VM create+start
	echo -e "${YELLOW}Setting up resources via API...${NC}"
	setup_output=$(python3 "${REPO_ROOT}/hack/setup_vm.py" \
		--kernel-path "$KERNEL_PATH" \
		--initramfs-path "$INITRAMFS_PATH" \
		--cmdline "$CMDLINE")

	# Parse key=value output from setup_vm.py
	eval "$setup_output"

	if [[ -n "$VM_ID" ]]; then
		vm_id="$VM_ID"

		# Assign the host-side IP on the qarax-node-managed TAP device.
		# qarax-node creates TAP devices with a deterministic name:
		# "qt" + first 8 hex chars of VM UUID (no dashes) + "n" + NIC index.
		tap_name="qt$(echo "${vm_id}" | tr -d '-' | cut -c1-8)n0"
		echo -e "${YELLOW}Configuring host network for SSH access (${tap_name})...${NC}"
		e2e_compose_cmd exec -T qarax-node \
			ip addr add 192.168.100.1/24 dev "${tap_name}" 2>/dev/null || true

		# Wait for VM SSH to become available (static IP: 192.168.100.2)
		echo -e "${YELLOW}Waiting for VM SSH to become available...${NC}"
		ssh_ready=0
		timeout=60
		elapsed=0
		while [[ $elapsed -lt $timeout ]]; do
			sleep 2
			elapsed=$((elapsed + 2))
			if e2e_compose_cmd exec -T qarax-node nc -z -w1 192.168.100.2 22 2>/dev/null; then
				ssh_ready=1
				echo -e "${GREEN}VM SSH is ready!${NC}"
				break
			fi
			if [[ $((elapsed % 10)) -eq 0 ]]; then
				echo -e "${YELLOW}  Still waiting... (${elapsed}s / ${timeout}s)${NC}"
			fi
		done
		if [[ $ssh_ready -eq 0 ]]; then
			echo -e "${YELLOW}SSH not ready yet - VM may still be booting${NC}"
		fi

		# Show VM access info
		echo ""
		echo -e "${GREEN}===== Example VM Ready =====${NC}"
		echo "VM ID: ${vm_id}"
		echo "Status: running"
		echo "Network: net0 (MAC: 52:54:00:12:34:56, TAP: ${tap_name})"
		echo "VM IP: 192.168.100.2 (static)"
		echo ""
		echo -e "${GREEN}SSH Access:${NC}"
		echo "  Username: root  |  Password: qarax"
		echo ""
		echo "SSH via nc ProxyCommand (from host):"
		echo "  ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ProxyCommand='docker compose -f e2e/docker-compose.yml exec -T qarax-node nc %h %p' root@192.168.100.2"
		echo ""
		echo "Or open a shell in the node and use nc to verify connectivity:"
		echo "  docker compose -f e2e/docker-compose.yml exec qarax-node nc -z 192.168.100.2 22 && echo 'SSH port open'"
		echo ""
		echo "View VM console output:"
		echo "  docker compose -f e2e/docker-compose.yml exec qarax-node tail -f /var/lib/qarax/vms/${vm_id}.console.log"
		echo ""
		echo "Stop VM:   qarax vm stop ${vm_id}"
		echo "Delete VM: qarax vm delete ${vm_id}"
		echo ""
	else
		echo -e "${RED}VM was not created. Check logs above for errors.${NC}"
	fi
else
	# No --with-vm: register host, init, and create overlaybd storage pool
	echo ""
	echo -e "${YELLOW}Setting up host and storage pools...${NC}"
	setup_output=$(python3 "${REPO_ROOT}/hack/setup_vm.py" \
		--skip-vm \
		--kernel-path /dev/null)
	eval "$setup_output"
	echo -e "${GREEN}Host registered and overlaybd storage pool created.${NC}"
fi

if [[ $WITH_VM -eq 0 ]]; then
	# Only show verbose instructions if --with-vm was not used
	echo ""
	echo -e "${GREEN}Qarax is running locally.${NC}"
	echo ""
	echo -e "${GREEN}✓ Ready to create VMs with networking and SSH access${NC}"
	echo "  Use: ./hack/run-local.sh --with-vm"
	echo ""
	print_api_endpoints
	print_command_section "Quick try (qarax):" \
		"cargo run -p cli vm list" \
		"cargo run -p cli host list"
	print_overlaybd_workflow
	echo "Legacy: create VM directly from OCI image ref (virtiofs or overlaybd auto-detected):"
	echo '  cargo run -p cli vm create --name alpine-vm --vcpus 1 --memory 268435456 \'
	echo '    --image-ref public.ecr.aws/docker/library/alpine:latest'
	echo '  cargo run -p cli vm start my-vm'
	echo ""
	print_command_section "Other useful commands:" \
		"cargo run -p cli vm get <VM_ID>" \
		"cargo run -p cli vm stop <VM_ID>" \
		"cargo run -p cli vm delete <VM_ID>" \
		"cargo run -p cli host init <HOST_ID>         # connect via gRPC, mark host UP" \
		"cargo run -p cli --json vm list              # raw JSON output" \
		"cargo run -p cli boot-source list" \
		"cargo run -p cli storage-pool list" \
		"cargo run -p cli transfer list --pool <POOL_ID>" \
		"cargo run -p cli job get <JOB_ID>"
	print_command_section "Docker stack commands:" \
		"docker compose -f e2e/docker-compose.yml logs -f    # Follow all logs" \
		"docker compose -f e2e/docker-compose.yml logs -f qarax-node" \
		"docker compose -f e2e/docker-compose.yml exec qarax-node sh" \
		"docker compose -f e2e/docker-compose.yml up -d --force-recreate qarax-node  # Apply device/compose changes" \
		"make stop-local                                      # Stop and remove stack + volumes"
else
	# With --with-vm (Alpine guest in container), show concise summary
	echo ""
	echo -e "${GREEN}Qarax stack ready.${NC}"
	echo ""
	echo "API:        http://localhost:8000/"
	echo "Swagger UI: http://localhost:8000/swagger-ui"
	echo ""
	echo "Cleanup: ./hack/run-local.sh --cleanup"
	echo ""
fi
