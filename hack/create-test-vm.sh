#!/usr/bin/env bash
#
# Simple script to create and start a test VM with networking.
# Prerequisites: qarax stack already running (via ./hack/run_local.sh)
#
# Usage:
#   ./hack/create-test-vm.sh [vm-name]
#

set -e

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VM_NAME="${1:-test-vm-$(date +%s)}"
API_URL="${QARAX_API:-http://localhost:8000}"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo "===== Creating Test VM: ${VM_NAME} ====="
echo ""

# 1. Create TAP device
echo -e "${YELLOW}[1/5] Creating TAP device...${NC}"
TAP_NAME="tap-$(date +%s | tail -c 5)"
docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" exec -T qarax-node sh -c "
  ip tuntap add ${TAP_NAME} mode tap
  ip link set ${TAP_NAME} up
  echo 'TAP device ${TAP_NAME} created and up'
" || {
  echo -e "${RED}Failed to create TAP device${NC}"
  exit 1
}

# 2. Get or create storage pool
echo -e "${YELLOW}[2/5] Getting or creating storage pool...${NC}"
pool_id=$(curl -s "${API_URL}/storage-pools" | grep -o '"id":"[^"]*"' | head -n1 | cut -d'"' -f4)

if [[ -z "$pool_id" ]]; then
  pool_id=$(curl -s -X POST "${API_URL}/storage-pools" \
    -H "Content-Type: application/json" \
    -d '{"name":"default-pool","pool_type":"local","config":{}}' | tr -d '"')
  echo "Created pool: ${pool_id}"
else
  echo "Using existing pool: ${pool_id}"
fi

# 3. Create kernel storage object
echo -e "${YELLOW}[3/5] Creating kernel storage object...${NC}"
kernel_id=$(curl -s -X POST "${API_URL}/storage-objects" \
  -H "Content-Type: application/json" \
  -d "{\"name\":\"kernel-${VM_NAME}\",\"storage_pool_id\":\"${pool_id}\",\"object_type\":\"kernel\",\"size_bytes\":20000000,\"config\":{\"path\":\"/var/lib/qarax/images/vmlinux\"}}" | tr -d '"')
echo "Kernel object: ${kernel_id}"

# 4. Create initramfs storage object
echo -e "${YELLOW}[4/5] Creating initramfs storage object...${NC}"
initramfs_id=$(curl -s -X POST "${API_URL}/storage-objects" \
  -H "Content-Type: application/json" \
  -d "{\"name\":\"initramfs-${VM_NAME}\",\"storage_pool_id\":\"${pool_id}\",\"object_type\":\"initrd\",\"size_bytes\":5000000,\"config\":{\"path\":\"/var/lib/qarax/images/test-initramfs.gz\"}}" | tr -d '"')
echo "Initramfs object: ${initramfs_id}"

# 5. Create boot source
echo -e "${YELLOW}[5/5] Creating boot source...${NC}"
boot_id=$(curl -s -X POST "${API_URL}/boot-sources" \
  -H "Content-Type: application/json" \
  -d "{\"name\":\"boot-${VM_NAME}\",\"description\":\"Boot source for ${VM_NAME}\",\"kernel_image_id\":\"${kernel_id}\",\"initrd_image_id\":\"${initramfs_id}\",\"kernel_params\":\"console=ttyS0\"}" | tr -d '"')
echo "Boot source: ${boot_id}"

# 6. Generate random MAC address
MAC="52:54:00:$(openssl rand -hex 3 | sed 's/\(..\)/\1:/g; s/:$//')"

# 7. Create VM with network interface
echo ""
echo -e "${YELLOW}Creating VM with network...${NC}"
vm_id=$(curl -s -X POST "${API_URL}/vms" \
  -H "Content-Type: application/json" \
  -d "{
    \"name\":\"${VM_NAME}\",
    \"hypervisor\":\"cloud_hv\",
    \"boot_vcpus\":1,
    \"max_vcpus\":1,
    \"memory_size\":268435456,
    \"boot_source_id\":\"${boot_id}\",
    \"networks\":[
      {
        \"id\":\"net0\",
        \"mac\":\"${MAC}\",
        \"tap\":\"${TAP_NAME}\"
      }
    ]
  }" | tr -d '"')

if [[ -z "$vm_id" ]] || [[ "$vm_id" == "null" ]]; then
  echo -e "${RED}Failed to create VM${NC}"
  exit 1
fi

echo -e "${GREEN}VM created: ${vm_id}${NC}"

# 8. Start the VM
echo ""
echo -e "${YELLOW}Starting VM...${NC}"
start_result=$(curl -s -X POST "${API_URL}/vms/${vm_id}/start")

echo -e "${GREEN}VM started successfully!${NC}"
echo ""
echo "===== VM Information ====="
echo "VM ID:      ${vm_id}"
echo "Name:       ${VM_NAME}"
echo "MAC:        ${MAC}"
echo "TAP Device: ${TAP_NAME}"
echo ""
echo "View console:"
echo "  docker compose -f e2e/docker-compose.yml exec qarax-node tail -f /var/lib/qarax/vms/${vm_id}.console.log"
echo ""
echo "Check network config:"
echo "  docker compose -f e2e/docker-compose.yml exec qarax-node curl -s --unix-socket /var/lib/qarax/vms/${vm_id}.sock http://localhost/api/v1/vm.info | jq '.config.net'"
echo ""
echo "Stop VM:"
echo "  curl -X POST ${API_URL}/vms/${vm_id}/stop"
echo ""
echo "Delete VM:"
echo "  curl -X DELETE ${API_URL}/vms/${vm_id}"
echo ""
