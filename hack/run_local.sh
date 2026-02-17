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
#   ./hack/run_local.sh            # Build and start the stack
#   ./hack/run_local.sh --with-vm  # Create and start a VM with SSH access
#   ./hack/run_local.sh --cleanup  # Stop and remove stack + volumes
#   REBUILD=1 ./hack/run_local.sh  # Rebuild Docker images from scratch
#   SKIP_BUILD=1 ./hack/run_local.sh # Use existing qarax-node binary
#
# After start:
#   API:        http://localhost:8000
#   Swagger UI: http://localhost:8000/swagger-ui
#

set -e

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Parse flags
WITH_VM=0
for arg in "$@"; do
  case $arg in
    --with-vm)
      WITH_VM=1
      shift
      ;;
    --cleanup)
      echo "===== Qarax local cleanup ====="
      cd "${REPO_ROOT}/e2e"
      echo -e "${YELLOW}Stopping and removing stack (postgres, qarax, qarax-node) and volumes...${NC}"
      docker compose down -v
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
  echo "  ./hack/run_local.sh --cleanup && ./hack/run_local.sh"
  echo ""
fi

# Build qarax-node binary for the node container (same as E2E)
NODE_BINARY="${REPO_ROOT}/target/x86_64-unknown-linux-musl/release/qarax-node"
if [[ -z "${SKIP_BUILD}" ]]; then
  if [[ -n "${REBUILD}" ]] || [[ ! -f "${NODE_BINARY}" ]]; then
    echo -e "${YELLOW}Building qarax-node (release, musl)...${NC}"
    cargo build --release -p qarax-node
  else
    echo -e "${GREEN}Using existing qarax-node binary${NC}"
  fi
else
  if [[ ! -f "${NODE_BINARY}" ]]; then
    echo -e "${RED}SKIP_BUILD=1 but ${NODE_BINARY} not found. Build it first or remove SKIP_BUILD.${NC}"
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

    cat > /tmp/build-rootfs-$$.sh << 'ROOTFS_SCRIPT'
#!/bin/sh
set -e
apk add --no-cache e2fsprogs wget >/dev/null 2>&1
echo "Creating 1GB rootfs image..."
dd if=/dev/zero of=/output/rootfs.img bs=1M count=1024
echo "Formatting with ext4..."
mkfs.ext4 -F /output/rootfs.img
echo "Mounting rootfs..."
mkdir -p /mnt/rootfs
mount -o loop /output/rootfs.img /mnt/rootfs
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
chmod 666 /output/rootfs.img
ls -lh /output/rootfs.img
ROOTFS_SCRIPT

    chmod +x /tmp/build-rootfs-$$.sh
    docker run --rm --privileged \
      -v "${BOOT_IMAGES_DIR}:/output" \
      -v "/tmp/build-rootfs-$$.sh:/build-rootfs.sh:ro" \
      alpine:3.19 sh /build-rootfs.sh
    rm -f /tmp/build-rootfs-$$.sh
    echo -e "${GREEN}Rootfs built: $ROOTFS_IMG${NC}"
  fi

  export PRODUCTION_IMAGES_DIR="$BOOT_IMAGES_DIR"
  export PRODUCTION_ROOTFS="/var/lib/qarax/production-images/rootfs.img"
fi

# Start stack (postgres + qarax + qarax-node)
echo -e "${YELLOW}Starting Docker stack...${NC}"
cd "${REPO_ROOT}/e2e"

if [[ -n "${REBUILD}" ]]; then
  docker compose build --no-cache
fi
docker compose up -d --build
# Recreate to pick up any environment variable changes
docker compose up -d --force-recreate qarax qarax-node

# Wait for services to be healthy
echo -e "${YELLOW}Waiting for services to be healthy...${NC}"
timeout=90
elapsed=0
while [[ $elapsed -lt $timeout ]]; do
  healthy_count=$(docker compose ps 2>/dev/null | grep -c '(healthy)' || echo "0")
  total_services=3

  if [[ "$healthy_count" -ge "$total_services" ]]; then
    echo ""
    echo -e "${GREEN}All services are healthy.${NC}"
    break
  fi

  if docker compose ps 2>/dev/null | grep -q "Exit"; then
    echo ""
    echo -e "${RED}A service has failed.${NC}"
    docker compose ps
    docker compose logs --tail=80
    exit 1
  fi

  echo -n "."
  sleep 2
  elapsed=$((elapsed + 2))
done

if [[ $elapsed -ge $timeout ]]; then
  echo ""
  echo -e "${RED}Timeout waiting for services.${NC}"
  docker compose ps
  docker compose logs --tail=80
  exit 1
fi

# Register the local qarax-node as a host (required for GET /hosts to show the node)
echo ""
echo -e "${YELLOW}Registering host (local-node at qarax-node:50051)...${NC}"
host_resp=$(curl -s -w "\n%{http_code}" -X POST http://localhost:8000/hosts \
  -H "Content-Type: application/json" \
  -d '{"name":"local-node","address":"qarax-node","port":50051,"host_user":"root","password":""}')
host_code=$(echo "$host_resp" | tail -n1)
if [[ "$host_code" == "201" ]]; then
  echo -e "${GREEN}Host registered.${NC}"
elif [[ "$host_code" == "409" ]]; then
  echo -e "${GREEN}Host already registered.${NC}"
else
  echo -e "${RED}Failed to register host (HTTP $host_code).${NC}"
  echo "$host_resp" | head -n -1
fi

# Set host status to "up" so GET /hosts shows it as up (VM ops use configured node either way)
host_id=$(curl -s http://localhost:8000/hosts | sed -n 's/.*"id":"\([^"]*\)","name":"local-node".*/\1/p')
if [[ -n "$host_id" ]]; then
  curl -s -X PATCH "http://localhost:8000/hosts/${host_id}" \
    -H "Content-Type: application/json" -d '{"status":"up"}' >/dev/null
  echo -e "${GREEN}Host status set to up.${NC}"
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
    KERNEL_VERSION=$(docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" exec -T qarax-node ls /lib/modules/ | head -1 | tr -d '\r')
    MODULE_DIR="/tmp/qarax-mods-$$"
    mkdir -p "$MODULE_DIR"

    # Extract network modules from qarax-node
    cat > /tmp/get-mods-$$.sh << GETMODS
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
    docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" exec -T qarax-node /tmp/get-mods.sh
    docker cp e2e-qarax-node-1:/tmp/mods/. "$MODULE_DIR/"

    # Build the initramfs inside the qarax-node container (uses Fedora busybox)
    docker cp "$MODULE_DIR/." e2e-qarax-node-1:/tmp/bootmods/
    docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" exec -T qarax-node sh -c '
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

  # 1. Get or create storage pool
  # Try to fetch existing pool first
  pool_id=$(curl -s http://localhost:8000/storage-pools | sed -n 's/.*"id":"\([^"]*\)","name":"local-pool".*/\1/p' | head -n1)

  if [[ -z "$pool_id" ]]; then
    # Pool doesn't exist, create it
    pool_resp=$(curl -s -w "\n%{http_code}" -X POST http://localhost:8000/storage-pools \
      -H "Content-Type: application/json" \
      -d '{"name":"local-pool","pool_type":"local","config":{}}')
    pool_code=$(echo "$pool_resp" | tail -n1)
    pool_id=$(echo "$pool_resp" | head -n -1 | tr -d '"')

    if [[ "$pool_code" == "201" ]]; then
      echo -e "${GREEN}Storage pool created: ${pool_id}${NC}"
    else
      echo -e "${RED}Failed to create storage pool (HTTP $pool_code)${NC}"
      echo "$pool_resp" | head -n -1
      pool_id=""
    fi
  else
    echo -e "${GREEN}Using existing storage pool: ${pool_id}${NC}"
  fi

  if [[ -n "$pool_id" ]]; then
    # 2. Get or create kernel storage object
    kernel_id=$(curl -s http://localhost:8000/storage-objects | sed -n 's/.*"id":"\([^"]*\)".*"name":"example-kernel".*/\1/p' | head -n1)

    if [[ -z "$kernel_id" ]]; then
      kernel_resp=$(curl -s -w "\n%{http_code}" -X POST http://localhost:8000/storage-objects \
        -H "Content-Type: application/json" \
        -d "{\"name\":\"example-kernel\",\"storage_pool_id\":\"${pool_id}\",\"object_type\":\"kernel\",\"size_bytes\":10000000,\"config\":{\"path\":\"${KERNEL_PATH}\"}}")
      kernel_code=$(echo "$kernel_resp" | tail -n1)
      kernel_id=$(echo "$kernel_resp" | head -n -1 | tr -d '"')

      if [[ "$kernel_code" == "201" ]]; then
        echo -e "${GREEN}Kernel storage object created: ${kernel_id}${NC}"
      else
        echo -e "${RED}Failed to create kernel storage object (HTTP $kernel_code)${NC}"
        echo "$kernel_resp" | head -n -1
        kernel_id=""
      fi
    else
      echo -e "${GREEN}Using existing kernel storage object: ${kernel_id}${NC}"
    fi

    # 3. Get or create initramfs storage object (only if using initramfs)
    initramfs_id=""
    if [[ -n "$INITRAMFS_PATH" ]]; then
      initramfs_id=$(curl -s http://localhost:8000/storage-objects | sed -n 's/.*"id":"\([^"]*\)".*"name":"example-initramfs".*/\1/p' | head -n1)

      if [[ -z "$initramfs_id" ]]; then
        initramfs_resp=$(curl -s -w "\n%{http_code}" -X POST http://localhost:8000/storage-objects \
          -H "Content-Type: application/json" \
          -d "{\"name\":\"example-initramfs\",\"storage_pool_id\":\"${pool_id}\",\"object_type\":\"initrd\",\"size_bytes\":5000000,\"config\":{\"path\":\"${INITRAMFS_PATH}\"}}")
        initramfs_code=$(echo "$initramfs_resp" | tail -n1)
        initramfs_id=$(echo "$initramfs_resp" | head -n -1 | tr -d '"')

        if [[ "$initramfs_code" == "201" ]]; then
          echo -e "${GREEN}Initramfs storage object created: ${initramfs_id}${NC}"
        else
          echo -e "${RED}Failed to create initramfs storage object (HTTP $initramfs_code)${NC}"
          echo "$initramfs_resp" | head -n -1
          initramfs_id=""
        fi
      else
        echo -e "${GREEN}Using existing initramfs storage object: ${initramfs_id}${NC}"
      fi
    fi

    # 4. Get or create boot source
    if [[ -n "$kernel_id" ]]; then
      boot_id=$(curl -s http://localhost:8000/boot-sources | sed -n 's/.*"id":"\([^"]*\)".*"name":"example-boot".*/\1/p' | head -n1)

      if [[ -z "$boot_id" ]]; then
        # Build boot source JSON - initrd_image_id is optional
        if [[ -n "$initramfs_id" ]]; then
          boot_json="{\"name\":\"example-boot\",\"description\":\"Example boot source\",\"kernel_image_id\":\"${kernel_id}\",\"initrd_image_id\":\"${initramfs_id}\",\"kernel_params\":\"${CMDLINE}\"}"
        else
          boot_json="{\"name\":\"example-boot\",\"description\":\"Example boot source\",\"kernel_image_id\":\"${kernel_id}\",\"kernel_params\":\"${CMDLINE}\"}"
        fi

        boot_resp=$(curl -s -w "\n%{http_code}" -X POST http://localhost:8000/boot-sources \
          -H "Content-Type: application/json" \
          -d "$boot_json")
        boot_code=$(echo "$boot_resp" | tail -n1)
        boot_id=$(echo "$boot_resp" | head -n -1 | tr -d '"')

        if [[ "$boot_code" == "201" ]]; then
          echo -e "${GREEN}Boot source created: ${boot_id}${NC}"
        else
          echo -e "${RED}Failed to create boot source (HTTP $boot_code)${NC}"
          echo "$boot_resp" | head -n -1
          boot_id=""
        fi
      else
        echo -e "${GREEN}Using existing boot source: ${boot_id}${NC}"
      fi

      # 5. Delete existing VM if present (to avoid stale state from previous runs)
      if [[ -n "$boot_id" ]]; then
        existing_vm_id=$(curl -s http://localhost:8000/vms | sed -n 's/.*"id":"\([^"]*\)".*"name":"example-vm".*/\1/p' | head -n1)

        if [[ -n "$existing_vm_id" ]]; then
          echo -e "${YELLOW}Deleting existing VM to avoid stale state: ${existing_vm_id}${NC}"
          delete_resp=$(curl -s -w "\n%{http_code}" -X DELETE "http://localhost:8000/vms/${existing_vm_id}")
          delete_code=$(echo "$delete_resp" | tail -n1)
          if [[ "$delete_code" == "204" ]] || [[ "$delete_code" == "200" ]]; then
            echo -e "${GREEN}Existing VM deleted${NC}"
          else
            echo -e "${YELLOW}Failed to delete existing VM (HTTP $delete_code), continuing anyway...${NC}"
          fi
          sleep 1
        fi

        # 6. Create TAP device for VM networking
        echo -e "${YELLOW}Creating TAP device for VM network...${NC}"
        docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" exec -T qarax-node sh -c '
          # Create TAP device if it doesn'\''t exist
          if ! ip link show tap0 >/dev/null 2>&1; then
            ip tuntap add tap0 mode tap
            ip link set tap0 up
            echo "TAP device tap0 created"
          else
            echo "TAP device tap0 already exists"
          fi
        ' || {
          echo -e "${RED}Failed to create TAP device${NC}"
          echo "Continuing anyway - VM may not have network connectivity"
        }

        # 6. Create fresh VM
        echo -e "${YELLOW}Creating fresh VM...${NC}"
        vm_json="{\"name\":\"example-vm\",\"hypervisor\":\"cloud_hv\",\"boot_vcpus\":1,\"max_vcpus\":1,\"memory_size\":268435456,\"boot_source_id\":\"${boot_id}\",\"networks\":[{\"id\":\"net0\",\"mac\":\"52:54:00:12:34:56\",\"tap\":\"tap0\"}]}"

        vm_resp=$(curl -s -w "\n%{http_code}" -X POST http://localhost:8000/vms \
          -H "Content-Type: application/json" \
          -d "$vm_json")
        vm_code=$(echo "$vm_resp" | tail -n1)
        vm_id=$(echo "$vm_resp" | head -n -1 | tr -d '"')

        if [[ "$vm_code" == "201" ]]; then
          echo -e "${GREEN}VM created: ${vm_id}${NC}"
        else
          echo -e "${RED}Failed to create VM (HTTP $vm_code)${NC}"
          echo "$vm_resp" | head -n -1
          vm_id=""
        fi

        if [[ -n "$vm_id" ]]; then
          # 7. Start the VM
          echo -e "${YELLOW}Starting VM...${NC}"
          start_resp=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:8000/vms/${vm_id}/start")
          start_code=$(echo "$start_resp" | tail -n1)

          if [[ "$start_code" == "200" ]]; then
            echo -e "${GREEN}VM started successfully!${NC}"

            # Configure host-side TAP device with IP for SSH access
            if [[ $WITH_VM -eq 1 ]]; then
              echo -e "${YELLOW}Configuring host network for SSH access...${NC}"
              docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" exec -T qarax-node sh -c '
                ip addr add 192.168.100.1/24 dev tap0 2>/dev/null || true
                echo "Host TAP device configured: 192.168.100.1"
              '
            fi

            if [[ $WITH_VM -eq 1 ]]; then
              # Wait for VM SSH to become available (static IP: 192.168.100.2)
              echo -e "${YELLOW}Waiting for VM SSH to become available...${NC}"
              ssh_ready=0
              timeout=60
              elapsed=0
              while [[ $elapsed -lt $timeout ]]; do
                sleep 2
                elapsed=$((elapsed + 2))
                if docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" exec -T qarax-node nc -z -w1 192.168.100.2 22 2>/dev/null; then
                  ssh_ready=1
                  echo -e "${GREEN}✓ VM SSH is ready!${NC}"
                  break
                fi
                if [[ $((elapsed % 10)) -eq 0 ]]; then
                  echo -e "${YELLOW}  Still waiting... (${elapsed}s / ${timeout}s)${NC}"
                fi
              done
              if [[ $ssh_ready -eq 0 ]]; then
                echo -e "${YELLOW}⚠ SSH not ready yet - VM may still be booting${NC}"
              fi
              vm_status="running"
            fi
          else
            echo -e "${RED}Failed to start VM (HTTP $start_code)${NC}"
            echo "$start_resp" | head -n -1
          fi
        fi

        # Show VM access info
        if [[ -n "$vm_id" ]]; then
          echo ""
          echo -e "${GREEN}===== Example VM Ready =====${NC}"
          echo "VM ID: ${vm_id}"
          echo "Status: ${vm_status:-running}"
          echo "Network: net0 (MAC: 52:54:00:12:34:56, TAP: tap0)"

          if [[ $WITH_VM -eq 1 ]]; then
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
          else
            echo ""
            echo -e "${GREEN}✓ VM is running with networking${NC}"
          fi

          echo ""
          echo "View VM console output:"
          echo "  docker compose -f e2e/docker-compose.yml exec qarax-node tail -f /var/lib/qarax/vms/${vm_id}.console.log"
          echo ""
          echo "Check network device in Cloud Hypervisor:"
          echo "  docker compose -f e2e/docker-compose.yml exec qarax-node curl -s --unix-socket /var/lib/qarax/vms/${vm_id}.sock http://localhost/api/v1/vm.info | jq '.config.net'"
          echo ""
          echo "Stop VM:"
          echo "  curl -X POST http://localhost:8000/vms/${vm_id}/stop"
          echo ""
          echo "Delete VM:"
          echo "  curl -X DELETE http://localhost:8000/vms/${vm_id}"
          echo ""
        fi
      fi
    fi
  fi
fi

if [[ $WITH_VM -eq 0 ]]; then
  # Only show verbose instructions if --with-vm was not used
  echo ""
  echo -e "${GREEN}Qarax is running locally.${NC}"
  echo ""
  echo -e "${GREEN}✓ Ready to create VMs with networking and SSH access${NC}"
  echo "  Use: ./hack/create-test-vm.sh [vm-name]"
  echo "  Or:  ./hack/run_local.sh --cleanup && ./hack/run_local.sh --with-vm"
  echo ""
  echo "Endpoints:"
  echo "  API (root):   http://localhost:8000/"
  echo "  Swagger UI:   http://localhost:8000/swagger-ui"
  echo "  OpenAPI JSON: http://localhost:8000/api-docs/openapi.json"
  echo ""
  echo "Quick try:"
  echo "  curl -s http://localhost:8000/vms"
  echo "  curl -s http://localhost:8000/hosts"
  echo ""
  echo "Create and start a VM (bash):"
  echo '  VM_ID=$(curl -s -X POST http://localhost:8000/vms \'
  echo '    -H "Content-Type: application/json" \'
  echo '    -d '\''{"name":"my-vm","hypervisor":"cloud_hv","boot_vcpus":1,"max_vcpus":1,"memory_size":268435456}'\'' )'
  echo '  curl -s -X POST "http://localhost:8000/vms/${VM_ID}/start"'
  echo ""
  echo "Create and start a VM (fish):"
  echo '  set VM_ID (curl -s -X POST http://localhost:8000/vms -H "Content-Type: application/json" -d '\''{"name":"my-vm","hypervisor":"cloud_hv","boot_vcpus":1,"max_vcpus":1,"memory_size":268435456}'\'')'
  echo '  curl -s -X POST "http://localhost:8000/vms/$VM_ID/start"'
  echo ""
  echo "Create a VM with a network interface (id + optional mac, tap, ip, mask):"
  echo '  curl -s -X POST http://localhost:8000/vms -H "Content-Type: application/json" \'
  echo '    -d '\''{"name":"my-vm-net","hypervisor":"cloud_hv","boot_vcpus":1,"max_vcpus":1,"memory_size":268435456,"networks":[{"id":"net0","mac":"52:54:00:12:34:56"}]}'\'''
  echo "  (In Docker, a pre-created tap may be required for real connectivity; see Swagger for full options.)"
  echo ""
  echo "If create fails, VM_ID is the error body (not a UUID), so start fails and GET /vms is []."
  echo "See why create failed (run this and check the output):"
  echo '  curl -s -w "\nHTTP %{http_code}\n" -X POST http://localhost:8000/vms \'
  echo '    -H "Content-Type: application/json" \'
  echo '    -d '\''{"name":"my-vm","hypervisor":"cloud_hv","boot_vcpus":1,"max_vcpus":1,"memory_size":268435456}'\'''
  echo "Then: docker compose -f e2e/docker-compose.yml logs qarax qarax-node"
  echo "Check qarax can reach qarax-node: docker compose -f e2e/docker-compose.yml exec qarax nc -zv qarax-node 50051"
  echo ""
  echo "Accessing a VM:"
  echo "  1. Start the VM first: curl -s -X POST http://localhost:8000/vms/<vm_id>/start"
  echo "  2. Serial output is written to /var/lib/qarax/vms/<vm_id>.console.log on the node."
  echo "  3. View it: docker compose -f e2e/docker-compose.yml exec qarax-node tail -f /var/lib/qarax/vms/<vm_id>.console.log"
  echo "  (Replace <vm_id> with the UUID from create or GET /vms. The log is empty until the VM is started.)"
  echo ""
  echo "Useful commands:"
  echo "  docker compose -f e2e/docker-compose.yml logs -f    # Follow all logs"
  echo "  docker compose -f e2e/docker-compose.yml logs -f qarax-node"
  echo "  docker compose -f e2e/docker-compose.yml exec qarax-node sh"
  echo "  docker compose -f e2e/docker-compose.yml up -d --force-recreate qarax-node  # Apply device/compose changes"
  echo "  ./hack/run_local.sh --cleanup                        # Stop and remove stack + volumes"
  echo ""
else
  # With --with-vm, show concise summary
  echo ""
  echo -e "${GREEN}Qarax stack ready.${NC}"
  echo ""
  echo "API:        http://localhost:8000/"
  echo "Swagger UI: http://localhost:8000/swagger-ui"
  echo ""
  echo "Useful commands:"
  echo "  docker compose -f e2e/docker-compose.yml logs -f qarax"
  echo "  docker compose -f e2e/docker-compose.yml logs -f qarax-node"
  echo "  docker compose -f e2e/docker-compose.yml exec qarax-node sh"
  echo "  docker compose -f e2e/docker-compose.yml exec qarax-node ls -la /var/lib/qarax/vms/"
  echo ""
  echo "Cleanup: ./hack/run_local.sh --cleanup"
  echo ""
fi
