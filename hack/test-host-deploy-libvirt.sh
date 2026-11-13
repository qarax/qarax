#!/usr/bin/env bash
#
# Run qarax-node in a libvirt VM, registered with the local control plane
# through the real host-deploy flow: `qarax host deploy` SSHes into the VM,
# runs `bootc switch` to the node image and reboots it.
#
# The node image is the appliance (deployments/Containerfile.qarax-vmm) plus a
# thin test layer: a password SSH login for the deploy, a fixed SSH host key,
# and plain-HTTP access to the compose registry it is pulled from. The VM
# first boots a disk that bootc-image-builder made from that image (cached),
# and is registered as host "libvirt-node".
#
# Usage:
#   test-host-deploy-libvirt.sh                    full deploy test on a new VM, destroyed on exit
#   test-host-deploy-libvirt.sh --keep-vm          leave the node running (make run-local VM=1)
#   test-host-deploy-libvirt.sh --keep-vm --fresh  recreate the kept VM instead of reusing it
#   test-host-deploy-libvirt.sh --start-stack      also start registry, postgres and qarax
#   test-host-deploy-libvirt.sh --cleanup          destroy the VM and delete its host record
#
# A kept VM is reused by the next run: if it already runs the current image,
# the run only makes sure its host is registered and UP; otherwise it
# redeploys onto it. Without --keep-vm a run replaces any kept VM.
#
# Needs libvirt (qemu:///system, network "default"), rootful podman through
# sudo, Docker Compose, jq, curl and ssh-keygen. Environment: API_URL,
# VM_NAME, VM_MEMORY_MB, VM_VCPUS, VM_DISK_SIZE, VM_MAC, HOST_NAME, SSH_USER,
# SSH_PASSWORD, LIBVIRT_BRIDGE_IP, LOCAL_REGISTRY_PORT, STATE_DIR, BIB_IMAGE
# (see docs/DEVELOPMENT.md).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${API_URL:-$SERVER}"
RELEASE_DIR="${REPO_ROOT}/target/${MUSL_TARGET}/release"

VM_NAME="${VM_NAME:-qarax-deploy-test}"
VM_MEMORY_MB="${VM_MEMORY_MB:-3072}"
VM_VCPUS="${VM_VCPUS:-2}"
VM_DISK_SIZE="${VM_DISK_SIZE:-20G}"
# A fixed MAC, so libvirt's DHCP hands a recreated VM the address its host
# record was registered with (qarax can't change a host's address).
VM_MAC="${VM_MAC:-52:54:00:71:a8:01}"
HOST_NAME="${HOST_NAME:-libvirt-node}"
SSH_USER="${SSH_USER:-qarax}"
SSH_PASSWORD="${SSH_PASSWORD:-qarax}"
# deployments/qarax-node.service
NODE_PORT=50051
# The compose registry's published port (e2e/docker-compose.yml).
LOCAL_REGISTRY_PORT="${LOCAL_REGISTRY_PORT:-5001}"

# On disk rather than in /tmp (tmpfs on many systems: the base disk and the
# VM's overlay would sit in RAM, and vanish under a kept VM on reboot), and
# under libvirt's image directory, which the qemu:///system emulator user can
# reach and which carries the right SELinux/AppArmor labels; a $HOME path is
# usually not traversable by that user.
STATE_DIR="${STATE_DIR:-/var/lib/libvirt/images/qarax-dev}"
BUILD_DIR="${STATE_DIR}/build"
VM_DISK="${STATE_DIR}/${VM_NAME}.qcow2"
# The image the VM runs, written after a successful deploy.
DEPLOYED_FILE="${STATE_DIR}/deployed-image"

APPLIANCE_REF="localhost/qarax-vmm-host:dev"
TEST_HOST_TAG="localhost/qarax-test-host:latest"
# Only "latest" is published (no version tags); this is the digest it pointed
# at from 2026-11-02 to 2026-11-09. Older digests stay pullable. To bump:
#   skopeo inspect --format '{{.Digest}}' docker://quay.io/centos-bootc/bootc-image-builder:latest
BIB_IMAGE="${BIB_IMAGE:-quay.io/centos-bootc/bootc-image-builder@sha256:4d8a0d9d173a9f450ff42d70777e1e0a6681a68f5c0f5084b65222eceb1f4d05}"

export LIBVIRT_DEFAULT_URI="${LIBVIRT_DEFAULT_URI:-qemu:///system}"

usage() {
	awk 'NR > 2 && /^#/ { sub(/^# ?/, ""); print; next } NR > 2 { exit }' "$0"
}

require_cmd() {
	local cmd
	for cmd in "$@"; do
		command -v "$cmd" >/dev/null || die "missing required command: ${cmd}"
	done
}

# ── Stack ────────────────────────────────────────────────────────────────────

ensure_release_binaries() {
	local bin missing=()
	for bin in qarax-server qarax-node qarax-init qarax; do
		[[ -x "${RELEASE_DIR}/${bin}" ]] || missing+=("$bin")
	done
	((${#missing[@]} == 0)) && return 0
	[[ "$START_STACK" -eq 1 ]] ||
		die "missing release binaries in ${RELEASE_DIR}: ${missing[*]}\nBuild them with: cargo build --release -p qarax -p qarax-node -p qarax-init -p cli (or re-run with --start-stack)"
	step "Building release binaries"
	(cd "$REPO_ROOT" && run cargo build --release -p qarax -p qarax-node -p qarax-init -p cli)
}

# qarax must run on host networking to reach the VM: libvirt's NAT network
# only accepts connections from the host itself, not from a Docker bridge.
ensure_vm_stack() {
	local build_flag=--no-build
	if ! server_reachable; then
		[[ "$START_STACK" -eq 1 ]] ||
			die "qarax API not reachable at ${SERVER}.\nStart it with 'make run-local VM=1', or re-run with --start-stack."
		build_flag=--build
	fi
	ensure_release_binaries
	step "Starting registry, postgres and qarax with the vm-mode overlay"
	# Compose recreates only services whose configuration changed, so this is
	# a no-op after 'make run-local VM=1'.
	run docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" -f "${REPO_ROOT}/e2e/docker-compose.vm-mode.yml" \
		up -d --wait "$build_flag" registry postgres qarax
	require_server
}

use_cli() {
	QARAX_BIN="$(find_qarax_bin)"
	[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found: cargo build --release -p cli"
}

# ── libvirt ──────────────────────────────────────────────────────────────────

ensure_libvirt_network() {
	local active
	active=$(virsh net-info default | awk '/^Active:/ { print $2 }') ||
		die "libvirt network 'default' not found on ${LIBVIRT_DEFAULT_URI}"
	[[ "$active" == yes ]] || run virsh net-start default
}

libvirt_bridge_ip() {
	local xml re="<ip [^>]*address='([0-9.]+)'"
	xml=$(virsh net-dumpxml default)
	[[ "$xml" =~ $re ]] || die "libvirt network 'default' has no IPv4 address; set LIBVIRT_BRIDGE_IP"
	echo "${BASH_REMATCH[1]}"
}

# grep without -q reads all of virsh's output, so pipefail can't see a SIGPIPE.
domain_exists() { virsh list --all --name | grep -xF "$VM_NAME" >/dev/null; }
domain_running() { virsh list --state-running --name | grep -xF "$VM_NAME" >/dev/null; }

destroy_vm() {
	if domain_exists; then
		if domain_running; then
			virsh destroy "$VM_NAME" >/dev/null
		fi
		virsh undefine "$VM_NAME" --nvram >/dev/null
	fi
	rm -f "$VM_DISK" "$DEPLOYED_FILE"
}

tcp_open() {
	# Connection refused/timed out is the expected answer while the VM boots.
	timeout 2 bash -c ": >/dev/tcp/$1/$2" 2>/dev/null
}

# Set VM_IP to the VM's DHCP lease that answers on SSH. A recreated VM keeps
# its MAC, so a lease of its previous incarnation can still be listed.
find_vm_ip() {
	local ip
	for ip in $(virsh domifaddr "$VM_NAME" --source lease | awk '$3 == "ipv4" { sub("/.*", "", $4); print $4 }'); do
		if tcp_open "$ip" 22; then
			VM_IP="$ip"
			return 0
		fi
	done
	return 1
}

# ── Images ───────────────────────────────────────────────────────────────────

# Print the ID of a rootful podman image, or nothing if it doesn't exist.
image_id() {
	if sudo podman image exists "$1"; then
		sudo podman image inspect --format '{{.Id}}' "$1"
	fi
}

# Remove the image TAG pointed at before this build, if the build replaced
# it (podman also drops its now-unused intermediate layers). Only this
# script's images: `podman image prune` would take every dangling root image.
remove_replaced_image() {
	local old="$1" tag="$2"
	[[ -n "$old" && "$old" != "$(image_id "$tag")" ]] || return 0
	# Housekeeping only; e.g. a container still using the old image keeps it.
	sudo podman rmi "$old" >/dev/null || warn "could not remove the previous ${tag} image ${old:0:12}"
}

write_test_layer() {
	mkdir -p "$BUILD_DIR"
	if [[ ! -f "${BUILD_DIR}/ssh_host_ed25519_key" ]]; then
		ssh-keygen -q -t ed25519 -N '' -C "$VM_NAME" -f "${BUILD_DIR}/ssh_host_ed25519_key"
	fi
	cat >"${BUILD_DIR}/Containerfile" <<EOF
# Generated by hack/test-host-deploy-libvirt.sh: what the libvirt test host
# needs on top of the node appliance.
FROM ${APPLIANCE_REF}

# Password SSH login with passwordless sudo for 'qarax host deploy'.
RUN useradd -m -G wheel -s /bin/bash ${SSH_USER} && \\
    echo '${SSH_USER}:${SSH_PASSWORD}' | chpasswd && \\
    echo '${SSH_USER} ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/${SSH_USER} && \\
    systemctl enable sshd

# One host key (generated once, next to this file) for every VM the script
# creates: a recreated VM gets the same address, and a new key there would
# fail the control plane's known_hosts check.
COPY ssh_host_ed25519_key ssh_host_ed25519_key.pub /etc/ssh/
RUN chmod 0600 /etc/ssh/ssh_host_ed25519_key && \\
    printf '%s\n' 'PasswordAuthentication yes' 'HostKey /etc/ssh/ssh_host_ed25519_key' \\
      > /etc/ssh/sshd_config.d/01-qarax-test.conf

# bootc pulls the deploy image from the compose registry over plain HTTP.
RUN mkdir -p /etc/containers/registries.conf.d && \\
    printf '%s\n' '[[registry]]' 'location = "${LOCAL_REGISTRY}"' 'insecure = true' \\
      > /etc/containers/registries.conf.d/01-qarax-test.conf
EOF
}

# Build the appliance (same build args as `make appliance-build`, which isn't
# used because its cargo step builds qarax-node on its own, with different
# feature unification than run-local's build, and so relinks it every time)
# and the test image on top. Sets DEPLOY_IMAGE, tagged by image ID: bootc
# switch to the ref a host already runs is a no-op, so every new image needs
# a new ref.
build_images() {
	local old_appliance old_test test_id
	old_appliance=$(image_id "$APPLIANCE_REF")
	old_test=$(image_id "$TEST_HOST_TAG")

	step "Building the node appliance image ${APPLIANCE_REF}"
	run sudo podman build -f "${REPO_ROOT}/deployments/Containerfile.qarax-vmm" \
		--build-arg "CLOUD_HYPERVISOR_VERSION=${CLOUD_HYPERVISOR_VERSION}" \
		--build-arg "FIRECRACKER_VERSION=${FIRECRACKER_VERSION}" \
		--build-arg QARAX_VERSION=dev \
		-t "$APPLIANCE_REF" "$REPO_ROOT"

	step "Building the test host image ${TEST_HOST_TAG}"
	write_test_layer
	run sudo podman build -t "$TEST_HOST_TAG" "$BUILD_DIR"

	remove_replaced_image "$old_test" "$TEST_HOST_TAG"
	remove_replaced_image "$old_appliance" "$APPLIANCE_REF"
	test_id=$(image_id "$TEST_HOST_TAG")
	DEPLOY_IMAGE="${LOCAL_REGISTRY}/qarax-test-host:${test_id:0:12}"
}

# The base disk only has to boot and provide /var, which `bootc switch` never
# touches; the deploy replaces the rest. So it is keyed by the image recipe and
# pins, not by the binaries: a code change doesn't rerun bootc-image-builder.
base_image_key() {
	{
		cat "${REPO_ROOT}/deployments/Containerfile.qarax-vmm" \
			"${REPO_ROOT}/versions/node-artifacts.env" \
			"${BUILD_DIR}/Containerfile" \
			"${BUILD_DIR}/ssh_host_ed25519_key.pub"
		echo "${CLOUD_HYPERVISOR_VERSION} ${FIRECRACKER_VERSION}"
	} | sha256sum | cut -c1-16
}

ensure_base_image() {
	BASE_IMAGE="${STATE_DIR}/base-$(base_image_key).qcow2"
	if [[ -f "$BASE_IMAGE" ]]; then
		info "Using the cached base disk ${BASE_IMAGE}"
		return 0
	fi

	step "Building the base disk with bootc-image-builder (a few minutes)"
	local out="${STATE_DIR}/bib-output"
	sudo rm -rf "$out"
	mkdir -p "$out"
	run sudo podman run --rm --privileged --security-opt label=type:unconfined_t \
		-v /var/lib/containers/storage:/var/lib/containers/storage \
		-v "${out}:/output" \
		"$BIB_IMAGE" build --type qcow2 "$TEST_HOST_TAG"
	# Base disks of older recipes: the only VM that could use one was just destroyed.
	find "$STATE_DIR" -maxdepth 1 -name 'base-*.qcow2' -delete
	sudo mv "${out}/qcow2/disk.qcow2" "$BASE_IMAGE"
	sudo chown "$(id -u):$(id -g)" "$BASE_IMAGE"
	sudo rm -rf "$out"
}

create_vm() {
	ensure_base_image
	step "Creating libvirt VM ${VM_NAME}"
	qemu-img create -q -f qcow2 -F qcow2 -b "$BASE_IMAGE" "$VM_DISK" "$VM_DISK_SIZE"
	run virt-install \
		--name "$VM_NAME" \
		--memory "$VM_MEMORY_MB" \
		--vcpus "$VM_VCPUS" \
		--cpu host-passthrough \
		--import \
		--disk "path=${VM_DISK},format=qcow2,bus=virtio" \
		--network "network=default,model=virtio,mac=${VM_MAC}" \
		--os-variant centos-stream10 \
		--graphics none \
		--noautoconsole \
		--rng /dev/urandom
}

# ── Host ─────────────────────────────────────────────────────────────────────

# Print FIELD of the host record, or nothing if it isn't registered.
host_field() {
	qarax host list -o json | jq -r --arg n "$HOST_NAME" --arg f "$1" '.[] | select(.name == $n) | .[$f] // empty'
}

host_settled() { [[ "$(host_field status)" != installing ]]; }

ensure_host() {
	local address
	address=$(host_field address)
	if [[ -z "$address" ]]; then
		run qarax host add --name "$HOST_NAME" --address "$VM_IP" --port "$NODE_PORT" --user "$SSH_USER" -o json
	elif [[ "$address" != "$VM_IP" ]]; then
		die "host ${HOST_NAME} is registered at ${address}, but VM ${VM_NAME} is at ${VM_IP}.\nqarax can neither change a host's address nor delete a host: reset the stack with 'make stop-local' and re-run."
	fi
}

deploy_node() {
	step "Deploying ${DEPLOY_IMAGE} to ${HOST_NAME} (${VM_IP})"
	run sudo podman push --tls-verify=false "$TEST_HOST_TAG" "$DEPLOY_IMAGE"
	run qarax host deploy "$HOST_NAME" --image "$DEPLOY_IMAGE" --ssh-password "$SSH_PASSWORD"
	info "bootc switch, reboot, wait for qarax-node: a few minutes"
	wait_until 600 host_settled ||
		die "host ${HOST_NAME} still installing after 600 s.\nLogs: docker compose -f e2e/docker-compose.yml logs qarax"

	local status
	status=$(host_field status)
	if [[ "$status" != up ]]; then
		warn "Deploy failed (status ${status}): $(host_field installation_error)"
		warn "Logs:  docker compose -f e2e/docker-compose.yml logs qarax"
		warn "VM:    ssh ${SSH_USER}@${VM_IP} (password ${SSH_PASSWORD}), virsh console ${VM_NAME}"
		[[ "$KEEP_VM" -eq 1 ]] || warn "Re-run with --keep-vm to keep the VM after a failure."
		die "host deploy failed"
	fi
	echo "$DEPLOY_IMAGE" >"$DEPLOYED_FILE"
}

# Remove the host record with its VM; a later run registers it again. The
# server refuses while VMs are still placed on the host and says which.
delete_host() {
	[[ -n "$(host_field id)" ]] || return 0
	qarax host delete "$HOST_NAME" >/dev/null ||
		warn "kept host ${HOST_NAME}: delete its VMs, then: qarax host delete ${HOST_NAME}"
}

cleanup() {
	destroy_vm
	info "Destroyed VM ${VM_NAME} (base disks stay cached in ${STATE_DIR})"
	# The stack may already be gone when cleaning up.
	if server_reachable; then
		use_cli
		delete_host
	fi
}

# ── Main ─────────────────────────────────────────────────────────────────────

START_STACK=0
KEEP_VM=0
FRESH=0
CLEANUP_ONLY=0
for arg in "$@"; do
	case "$arg" in
	--start-stack) START_STACK=1 ;;
	--keep-vm) KEEP_VM=1 ;;
	--fresh) FRESH=1 ;;
	--cleanup) CLEANUP_ONLY=1 ;;
	-h | --help)
		usage
		exit 0
		;;
	*) die "unknown option: ${arg} (see --help)" ;;
	esac
done
# Without --keep-vm a run is a full deploy test, so it always starts from a new VM.
[[ "$KEEP_VM" -eq 1 ]] || FRESH=1

require_cmd virsh virt-install qemu-img podman sudo docker jq curl ssh-keygen timeout

if [[ "$CLEANUP_ONLY" -eq 1 ]]; then
	cleanup
	exit 0
fi

banner "qarax-node in libvirt VM ${VM_NAME}"
ensure_libvirt_network
LIBVIRT_BRIDGE_IP="${LIBVIRT_BRIDGE_IP:-$(libvirt_bridge_ip)}"
LOCAL_REGISTRY="${LIBVIRT_BRIDGE_IP}:${LOCAL_REGISTRY_PORT}"
ensure_vm_stack
use_cli

sudo install -d -o "$(id -u)" -g "$(id -g)" -m 0755 "$STATE_DIR"
build_images

if [[ "$KEEP_VM" -eq 0 ]]; then
	trap cleanup EXIT
fi
# A domain without its disk in STATE_DIR lost it, or was made by an older
# version of this script (disk in /tmp): recreate it.
if [[ "$FRESH" -eq 1 ]] || ! domain_exists || [[ ! -f "$VM_DISK" ]]; then
	destroy_vm
	create_vm
elif ! domain_running; then
	run virsh start "$VM_NAME"
fi

step "Waiting for ${VM_NAME} to answer on SSH"
wait_until 300 find_vm_ip || die "VM ${VM_NAME} has no DHCP lease answering on SSH after 300 s.\nCheck: virsh domifaddr ${VM_NAME} --source lease; virsh console ${VM_NAME}"
ok "VM address ${VM_IP}"
ensure_host

if [[ -f "$DEPLOYED_FILE" && "$(<"$DEPLOYED_FILE")" == "$DEPLOY_IMAGE" ]]; then
	step "VM already runs ${DEPLOY_IMAGE}: skipping the deploy (--fresh redoes it)"
	wait_until 120 tcp_open "$VM_IP" "$NODE_PORT" ||
		die "qarax-node isn't listening on ${VM_IP}:${NODE_PORT}; check 'sudo journalctl -u qarax-node' in the VM, or re-run with --fresh"
else
	deploy_node
fi

# Connects over gRPC, records the node's versions and marks the host UP.
run qarax host init "$HOST_NAME"
[[ "$(host_field status)" == up ]] || die "host ${HOST_NAME} is not UP after init"

echo
ok "Host ${HOST_NAME} is UP: qarax-node in VM ${VM_NAME} (${VM_IP}), image ${DEPLOY_IMAGE}"
info "SSH:      ssh ${SSH_USER}@${VM_IP}   (password ${SSH_PASSWORD})"
info "Console:  virsh -c ${LIBVIRT_DEFAULT_URI} console ${VM_NAME}"
if [[ "$KEEP_VM" -eq 1 ]]; then
	info "Remove:   bash hack/test-host-deploy-libvirt.sh --cleanup"
else
	info "The VM is destroyed on exit; --keep-vm keeps it."
fi
