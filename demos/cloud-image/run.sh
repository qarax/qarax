#!/usr/bin/env bash
#
# Demo: Boot a VM from a cloud image with cloud-init credential injection
#
# This script demonstrates the cloud image workflow:
#   1. Create a local storage pool
#   2. Download a cloud image (e.g. Ubuntu 22.04) directly into the pool
#   3. Create a VM with the image as its root disk + cloud-init to inject SSH keys
#   4. Start the VM
#
# The result is a fully functional VM with a standalone raw disk — no OCI registry,
# no OverlayBD, no external dependency after the download.
#
# Prerequisites:
#   - qarax stack running (make run-local or hack/run-local.sh)
#   - qarax CLI on PATH
#   - A host registered and in "up" state
#   - Internet access from the qarax-node for the image download
#   - An SSH public key at ~/.ssh/id_rsa.pub (or set SSH_PUB_KEY)
#
# Usage:
#   ./demos/cloud-image/run.sh
#   ./demos/cloud-image/run.sh --image-url https://example.com/custom.img --name my-vm
#   ./demos/cloud-image/run.sh --preallocate   # reserve blocks upfront
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

# Defaults
VM_NAME="demo-cloud-image-vm"
HOST_NAME="${QARAX_HOST:-local-node}"
POOL_NAME="demo-cloud-image-pool"
POOL_PATH="/var/lib/qarax/cloud-image-pool"
DISK_NAME="ubuntu-22.04-cloud"
IMAGE_URL="https://cloud-images.ubuntu.com/jammy/current/jammy-server-cloudimg-amd64.img"
VCPUS=2
MEMORY_MIB=1024
SERVER="${QARAX_SERVER:-http://localhost:8000}"
SSH_PUB_KEY="${SSH_PUB_KEY:-$(cat ~/.ssh/id_rsa.pub 2>/dev/null || echo '')}"
PREALLOCATE=false

# Parse arguments
while [[ $# -gt 0 ]]; do
	case $1 in
	--name)
		VM_NAME="$2"
		shift 2
		;;
	--pool-name)
		POOL_NAME="$2"
		shift 2
		;;
	--pool-path)
		POOL_PATH="$2"
		shift 2
		;;
	--disk-name)
		DISK_NAME="$2"
		shift 2
		;;
	--image-url)
		IMAGE_URL="$2"
		shift 2
		;;
	--host)
		HOST_NAME="$2"
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
	--ssh-key)
		SSH_PUB_KEY="$2"
		shift 2
		;;
	--preallocate)
		PREALLOCATE=true
		shift
		;;
	--server)
		SERVER="$2"
		shift 2
		;;
	*)
		echo "Unknown argument: $1" >&2
		exit 1
		;;
	esac
done

export QARAX_SERVER="$SERVER"

QARAX_BIN="$(find_qarax_bin)"
[[ -z "$QARAX_BIN" ]] && die "qarax CLI not found. Run 'make build' or add it to PATH."
QARAX="$QARAX_BIN --server $SERVER"

ensure_stack "$SERVER"

if [[ -z "$SSH_PUB_KEY" ]]; then
	echo "Error: no SSH public key found. Set SSH_PUB_KEY or ensure ~/.ssh/id_rsa.pub exists." >&2
	exit 1
fi

echo "=== Cloud Image VM Demo ==="
echo "  Image URL:  $IMAGE_URL"
echo "  VM name:    $VM_NAME"
echo "  Pool:       $POOL_NAME ($POOL_PATH)"
echo "  Preallocate: $PREALLOCATE"
echo ""

# ── Step 1: Storage pool ──────────────────────────────────────────────────────

echo "→ Creating storage pool '$POOL_NAME'..."
POOL_ID=$($QARAX storage-pool create \
	--name "$POOL_NAME" \
	--pool-type local \
	--config "{\"path\":\"$POOL_PATH\"}" \
	--host "$HOST_NAME" \
	--output json | jq -r '.pool_id' 2>/dev/null ||
	$QARAX storage-pool list --output json | jq -r ".[] | select(.name==\"$POOL_NAME\") | .id")

echo "  Pool: $POOL_ID"

# ── Step 2: Download cloud image into the pool ────────────────────────────────

echo "→ Downloading cloud image into pool (this may take a few minutes)..."

PREALLOCATE_FLAG=""
if [[ "$PREALLOCATE" == "true" ]]; then
	PREALLOCATE_FLAG="--preallocate"
fi

DISK_RESULT=$($QARAX storage-pool create-disk \
	--pool "$POOL_NAME" \
	--name "$DISK_NAME" \
	--source "$IMAGE_URL" \
	$PREALLOCATE_FLAG \
	--output json)

DISK_ID=$(echo "$DISK_RESULT" | jq -r '.storage_object_id')
echo "  Disk: $DISK_ID"

# ── Step 3: Create VM with the disk as root + cloud-init ──────────────────────

echo "→ Creating VM '$VM_NAME'..."

USER_DATA="#cloud-config
users:
  - name: qarax
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys:
      - $SSH_PUB_KEY
growpart:
  mode: auto
  devices: ['/']
resize_rootfs: true"

VM_ID=$($QARAX vm create \
	--name "$VM_NAME" \
	--vcpus "$VCPUS" \
	--memory "${MEMORY_MIB}MiB" \
	--root-disk "$DISK_NAME" \
	--cloud-init-user-data "$USER_DATA" \
	--output json | jq -r '.vm_id')

echo "  VM: $VM_ID"

# ── Step 4: Start VM ──────────────────────────────────────────────────────────

echo "→ Starting VM..."
$QARAX vm start "$VM_NAME"

echo ""
echo "=== Done ==="
echo "VM '$VM_NAME' is booting."
echo "cloud-init will set up the 'qarax' user with your SSH key on first boot."
echo ""
echo "Connect once the VM has an IP:"
echo "  ssh qarax@<vm-ip>"
echo ""
echo "To view the VM:"
echo "  $QARAX vm get $VM_NAME"
