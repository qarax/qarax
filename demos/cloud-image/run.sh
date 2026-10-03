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
#   - qarax stack running (make run-local), or let the script start it
#   - qarax CLI on PATH or built under target/ (make build)
#   - jq
#   - A host registered and in "up" state
#   - Internet access from the qarax-node for the image download
#   - An SSH public key at ~/.ssh/id_ed25519.pub or ~/.ssh/id_rsa.pub (or set SSH_PUB_KEY)
#
# Usage:
#   ./demos/cloud-image/run.sh
#   ./demos/cloud-image/run.sh --image-url https://example.com/custom.img --name my-vm
#   ./demos/cloud-image/run.sh --preallocate   # reserve blocks upfront
#   ./demos/cloud-image/run.sh --cleanup       # delete VM, disk, network, and pool
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

# Defaults
VM_NAME="demo-cloud-image-vm"
HOST_NAME="${QARAX_HOST:-local-node}"
POOL_NAME="demo-cloud-image-pool"
POOL_PATH="/var/lib/qarax/cloud-image-pool"
NETWORK_NAME="demo-cloud-image-net"
NETWORK_SUBNET="10.97.0.0/24"
NETWORK_GATEWAY="10.97.0.1"
BRIDGE_NAME="qci0"
PARENT_INTERFACE=""
DISK_NAME="ubuntu-22.04-cloud"
IMAGE_URL="https://cloud-images.ubuntu.com/minimal/releases/jammy/release/ubuntu-22.04-minimal-cloudimg-amd64.img"
VCPUS=2
MEMORY_GIB=1
SERVER="${QARAX_SERVER:-http://localhost:8000}"
SSH_PUB_KEY="${SSH_PUB_KEY:-$(cat ~/.ssh/id_ed25519.pub 2>/dev/null || cat ~/.ssh/id_rsa.pub 2>/dev/null || true)}"
PREALLOCATE=false
DOWNLOAD_TIMEOUT=1800
CLEANUP=0

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
	--network-name)
		NETWORK_NAME="$2"
		shift 2
		;;
	--subnet)
		NETWORK_SUBNET="$2"
		shift 2
		;;
	--gateway)
		NETWORK_GATEWAY="$2"
		shift 2
		;;
	--bridge-name)
		BRIDGE_NAME="$2"
		shift 2
		;;
	--parent-interface)
		PARENT_INTERFACE="$2"
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
		MEMORY_GIB="$2"
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
	--cleanup)
		CLEANUP=1
		shift
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo ""
		echo "Options:"
		echo "  --name NAME              VM name (default: demo-cloud-image-vm)"
		echo "  --image-url URL          Cloud image URL (default: Ubuntu 22.04 minimal)"
		echo "  --disk-name NAME         Disk storage object name (default: ubuntu-22.04-cloud)"
		echo "  --preallocate            Reserve disk blocks upfront (default: sparse)"
		echo "  --pool-name NAME         Local storage pool name (default: demo-cloud-image-pool)"
		echo "  --pool-path PATH         Pool directory on qarax-node (default: /var/lib/qarax/cloud-image-pool)"
		echo "  --network-name NAME      Network name (default: demo-cloud-image-net)"
		echo "  --subnet CIDR            Network subnet (default: 10.97.0.0/24)"
		echo "  --gateway IP             Network gateway (default: 10.97.0.1)"
		echo "  --bridge-name NAME       Bridge device on the host (default: qci0)"
		echo "  --parent-interface NIC   Bridge this host NIC instead of an isolated NAT bridge"
		echo "  --host NAME              Host for the pool and network (default: \$QARAX_HOST or local-node)"
		echo "  --vcpus N                Number of vCPUs (default: 2)"
		echo "  --memory GiB             Memory in GiB (default: 1)"
		echo "  --ssh-key KEY            SSH public key to inject (default: \$SSH_PUB_KEY or ~/.ssh/id_{ed25519,rsa}.pub)"
		echo "  --server URL             qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --cleanup                Delete the VM, disk, network, and pool, then exit"
		exit 0
		;;
	*)
		echo "Unknown argument: $1" >&2
		exit 1
		;;
	esac
done

export QARAX_SERVER="$SERVER"

command -v jq >/dev/null || die "jq is required"

QARAX_BIN="$(find_qarax_bin)"
[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found. Run 'make build' or add it to PATH."
QARAX="$QARAX_BIN --server $SERVER"

if [[ "$CLEANUP" -eq 1 ]]; then
	if ! curl -sf --max-time 3 "${SERVER}/" >/dev/null 2>&1; then
		echo "Stack not running — nothing to clean up."
		exit 0
	fi
	require_auth "$SERVER"
	echo "=== Cleaning up cloud-image demo resources ==="
	timeout 60 $QARAX vm force-stop --wait "$VM_NAME" >/dev/null 2>&1 || true
	$QARAX vm delete "$VM_NAME" 2>/dev/null || true
	$QARAX storage-object delete "$DISK_NAME" 2>/dev/null || true
	$QARAX network detach-host --network "$NETWORK_NAME" --host "$HOST_NAME" 2>/dev/null || true
	$QARAX network delete "$NETWORK_NAME" 2>/dev/null || true
	$QARAX storage-pool delete "$POOL_NAME" 2>/dev/null || true
	echo "Done."
	exit 0
fi

ensure_stack "$SERVER"

wait_for_job() {
	local job_id="$1"
	local label="$2"
	local timeout="$3"
	local elapsed=0

	while ((elapsed < timeout)); do
		local job_json status progress
		job_json=$($QARAX job get "$job_id" --output json)
		status=$(jq -r '.status' <<<"$job_json")
		case "$status" in
		completed)
			echo -e "\r  ${label}: completed          "
			return 0
			;;
		failed)
			echo ""
			die "${label} failed: $(jq -r '.error // "unknown error"' <<<"$job_json")"
			;;
		*)
			progress=$(jq -r '.progress // 0' <<<"$job_json")
			echo -ne "\r  ${label}: [${status}] ${progress}% (${elapsed}s)   "
			sleep 5
			elapsed=$((elapsed + 5))
			;;
		esac
	done
	echo ""
	die "${label} did not finish within ${timeout}s (job ${job_id}). Check: qarax job get ${job_id}"
}

if [[ -z "$SSH_PUB_KEY" ]]; then
	die "no SSH public key found. Set SSH_PUB_KEY, pass --ssh-key, or create ~/.ssh/id_ed25519.pub."
fi

if $QARAX vm get "$VM_NAME" >/dev/null 2>&1; then
	die "VM '$VM_NAME' already exists. Run '$0 --cleanup' first or pass --name."
fi

echo "=== Cloud Image VM Demo ==="
echo "  Image URL:  $IMAGE_URL"
echo "  VM name:    $VM_NAME"
echo "  Pool:       $POOL_NAME ($POOL_PATH)"
echo "  Network:    $NETWORK_NAME ($NETWORK_SUBNET)"
echo "  Memory:     ${MEMORY_GIB} GiB"
echo "  Preallocate: $PREALLOCATE"
echo ""

# ── Step 1: Storage pool ──────────────────────────────────────────────────────

if POOL_ID=$($QARAX storage-pool get "$POOL_NAME" --output json 2>/dev/null | jq -r '.id'); then
	echo "→ Reusing storage pool '$POOL_NAME'"
else
	echo "→ Creating storage pool '$POOL_NAME'..."
	POOL_ID=$($QARAX storage-pool create \
		--name "$POOL_NAME" \
		--pool-type local \
		--path "$POOL_PATH" \
		--host "$HOST_NAME" \
		--output json | jq -r '.pool_id')
fi

echo "  Pool: $POOL_ID"

# ── Step 2: Network ────────────────────────────────────────────────────────────

NETWORK_REUSED=0
if NETWORK_ID=$($QARAX network get "$NETWORK_NAME" --output json 2>/dev/null | jq -r '.id'); then
	echo "→ Reusing network '$NETWORK_NAME'"
	NETWORK_REUSED=1
else
	echo "→ Creating network '$NETWORK_NAME'..."
	NETWORK_ID=$($QARAX network create \
		--name "$NETWORK_NAME" \
		--subnet "$NETWORK_SUBNET" \
		--gateway "$NETWORK_GATEWAY" \
		--output json | jq -r '.network_id')
fi

echo "  Network: $NETWORK_ID"

echo "→ Attaching network to host '$HOST_NAME'..."
ATTACH_ARGS=(--network "$NETWORK_NAME" --host "$HOST_NAME" --bridge-name "$BRIDGE_NAME")
if [[ -n "$PARENT_INTERFACE" ]]; then
	ATTACH_ARGS+=(--parent-interface "$PARENT_INTERFACE")
fi
if [[ "$NETWORK_REUSED" -eq 1 ]]; then
	# The bridge already exists on the node after a previous run, so a second
	# attach fails; that is expected.
	$QARAX network attach-host "${ATTACH_ARGS[@]}" 2>/dev/null ||
		echo "  (already attached from a previous run)"
else
	$QARAX network attach-host "${ATTACH_ARGS[@]}"
fi

# ── Step 3: Download cloud image into the pool ────────────────────────────────

if DISK_ID=$($QARAX storage-object get "$DISK_NAME" --output json 2>/dev/null | jq -r '.id'); then
	echo "→ Reusing disk '$DISK_NAME' from a previous run (use --cleanup for a fresh download)"
	echo "  Disk: $DISK_ID"
else
	echo "→ Downloading cloud image into pool (this may take a few minutes)..."

	CREATE_DISK_ARGS=(--pool "$POOL_NAME" --name "$DISK_NAME" --source "$IMAGE_URL")
	if [[ "$PREALLOCATE" == "true" ]]; then
		CREATE_DISK_ARGS+=(--preallocate)
	fi

	DISK_RESULT=$($QARAX storage-pool create-disk "${CREATE_DISK_ARGS[@]}" --output json)

	DISK_ID=$(jq -r '.storage_object_id' <<<"$DISK_RESULT")
	DISK_JOB_ID=$(jq -r '.job_id // empty' <<<"$DISK_RESULT")
	echo "  Disk: $DISK_ID"
	if [[ -n "$DISK_JOB_ID" ]]; then
		wait_for_job "$DISK_JOB_ID" "Disk download" "$DOWNLOAD_TIMEOUT"
	fi
fi

# ── Step 4: Create VM with the disk as root + cloud-init ──────────────────────

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
resize_rootfs: true
runcmd:
  - [sh, -c, 'echo demo-cloud-image > /var/tmp/index.html']
  - [sh, -c, 'nohup python3 -m http.server 8080 --directory /var/tmp >/var/log/qarax-demo-http.log 2>&1 &']"

USER_DATA_FILE="$(mktemp /tmp/qarax-cloud-init-XXXXXX.yaml)"
trap 'rm -f "$USER_DATA_FILE"' EXIT
printf '%s\n' "$USER_DATA" >"$USER_DATA_FILE"

VM_ID=$($QARAX vm create \
	--name "$VM_NAME" \
	--vcpus "$VCPUS" \
	--memory "${MEMORY_GIB}GiB" \
	--boot-mode firmware \
	--root-disk "$DISK_ID" \
	--network "$NETWORK_NAME" \
	--cloud-init-user-data "$USER_DATA_FILE" \
	--output json | jq -r '.vm_id')

echo "  VM: $VM_ID"

# ── Step 5: Start VM ──────────────────────────────────────────────────────────

echo "→ Starting VM..."
$QARAX vm start "$VM_NAME"
VM_IP=$($QARAX network list-ips "$NETWORK_NAME" --output json |
	jq -r ".[] | select(.vm_id==\"$VM_ID\") | .ip_address" |
	head -n1)
VM_IP="${VM_IP%/32}"

echo ""
echo "=== Done ==="
echo "VM '$VM_NAME' is booting."
echo "cloud-init will set up the 'qarax' user with your SSH key on first boot and"
echo "start a tiny HTTP server on port 8080."
echo ""
if [[ -n "$VM_IP" ]]; then
	echo "Allocated IP: $VM_IP"
fi
echo ""
if [[ -n "$PARENT_INTERFACE" ]]; then
	echo "The network is bridged to '$PARENT_INTERFACE', so the VM should be reachable"
	echo "from your LAN once cloud-init finishes:"
	echo "  curl http://$VM_IP:8080"
	echo "  ssh qarax@$VM_IP"
else
	echo "Compose mode note: Qarax bridge networks live inside the qarax-node namespace."
	echo "Verify reachability from the node container:"
	echo "  docker compose -f e2e/docker-compose.yml exec qarax-node bash -c 'timeout 3 bash -c \"</dev/tcp/$VM_IP/22\" && echo ssh-open'"
	echo "  docker compose -f e2e/docker-compose.yml exec qarax-node curl http://$VM_IP:8080"
fi
echo ""
echo "To inspect the VM:"
echo "  qarax vm get $VM_NAME"
echo "  qarax vm console $VM_NAME"
echo ""
echo "To delete everything this demo created:"
echo "  $0 --cleanup"
