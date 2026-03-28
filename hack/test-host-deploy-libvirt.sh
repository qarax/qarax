#!/usr/bin/env bash
#
# Run a real local host-deploy test using a libvirt VM.
#
# What this does:
#   1) Builds a bootc container image with test fixtures (SSH, qarax-node stub)
#   2) Uses bootc-image-builder to create a qcow2 disk from that image
#   3) Creates a libvirt VM from the disk
#   4) Registers the VM as a host in qarax
#   5) Calls POST /hosts/{host_id}/deploy with reboot=true
#   6) Waits for host status to become UP
#
# Usage:
#   bash hack/test-host-deploy-libvirt.sh
#   bash hack/test-host-deploy-libvirt.sh --start-stack
#   bash hack/test-host-deploy-libvirt.sh --keep-vm
#   bash hack/test-host-deploy-libvirt.sh --cleanup
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CH_VERSION_FILE="${REPO_ROOT}/versions/cloud-hypervisor-version"
export CLOUD_HYPERVISOR_VERSION="${CLOUD_HYPERVISOR_VERSION:-$(tr -d '\n' <"$CH_VERSION_FILE")}"

API_URL="${API_URL:-http://localhost:8000}"
VM_NAME="${VM_NAME:-qarax-deploy-test}"
VM_MEMORY_MB="${VM_MEMORY_MB:-3072}"
VM_VCPUS="${VM_VCPUS:-2}"
VM_DISK_SIZE="${VM_DISK_SIZE:-20G}"

SSH_USER="${SSH_USER:-qarax}"
SSH_PASSWORD="${SSH_PASSWORD:-qarax}"
SSH_PORT="${SSH_PORT:-22}"
NODE_PORT="${NODE_PORT:-50051}"

# Bridge IP of the libvirt default network — reachable from the VM
LIBVIRT_BRIDGE_IP="${LIBVIRT_BRIDGE_IP:-192.168.122.1}"
# Local registry port (matches the e2e compose stack: registry exposed on host port 5001)
LOCAL_REGISTRY_PORT="${LOCAL_REGISTRY_PORT:-5001}"
LOCAL_REGISTRY="${LIBVIRT_BRIDGE_IP}:${LOCAL_REGISTRY_PORT}"

# Default deploy image: push the locally-built test image to the local registry
# so the VM can pull it without hitting public registries (avoids rate limits).
DEPLOY_IMAGE="${DEPLOY_IMAGE:-${LOCAL_REGISTRY}/qarax-test-host:latest}"
VM_BOOTC_IMAGE="${VM_BOOTC_IMAGE:-quay.io/centos-bootc/centos-bootc:stream10}"
TEST_HOST_TAG="${TEST_HOST_TAG:-localhost/qarax-test-host}"

export LIBVIRT_DEFAULT_URI="${LIBVIRT_DEFAULT_URI:-qemu:///system}"

COMPOSE_FILE="${REPO_ROOT}/e2e/docker-compose.yml"
LIBVIRT_OVERLAY="${REPO_ROOT}/e2e/docker-compose.libvirt.yml"

STATE_DIR="${STATE_DIR:-/tmp/qarax-libvirt-deploy}"
BASE_IMAGE="${STATE_DIR}/base-bootc.qcow2"
VM_DISK="${STATE_DIR}/${VM_NAME}.qcow2"

START_STACK=0
KEEP_VM=0
CLEANUP_ONLY=0

while [[ $# -gt 0 ]]; do
	case "$1" in
	--start-stack)
		START_STACK=1
		shift
		;;
	--keep-vm)
		KEEP_VM=1
		shift
		;;
	--cleanup)
		CLEANUP_ONLY=1
		shift
		;;
	*)
		echo "Unknown argument: $1" >&2
		exit 1
		;;
	esac
done

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "Missing required command: $1" >&2
		exit 1
	fi
}

wait_for_http() {
	local url="$1"
	local timeout_s="${2:-60}"
	local start_ts
	start_ts="$(date +%s)"
	while true; do
		if curl -fsS "${url}" >/dev/null 2>&1; then
			return 0
		fi
		if (("$(date +%s)" - start_ts >= timeout_s)); then
			echo "Timed out waiting for ${url}" >&2
			return 1
		fi
		sleep 2
	done
}

ensure_stack() {
	if ! wait_for_http "${API_URL}/" 2; then
		if [[ "${START_STACK}" -ne 1 ]]; then
			echo "qarax API is not reachable at ${API_URL}." >&2
			echo "Start it first (e.g. ./hack/run-local.sh) or re-run with --start-stack." >&2
			exit 1
		fi

		echo "Starting local qarax stack..."
		bash "${REPO_ROOT}/hack/run-local.sh"
		wait_for_http "${API_URL}/" 120
	fi

	# Apply host-network overlay so the qarax container can reach the libvirt VM bridge.
	# Only force-recreate the services that the overlay actually changes (postgres, qarax).
	# Recreating nfs-server is avoided because its kernel nfsd threads cannot be stopped
	# by Docker once the container is running.
	echo "Applying host-network overlay for libvirt connectivity..."
	docker compose -f "${COMPOSE_FILE}" -f "${LIBVIRT_OVERLAY}" \
		up -d --force-recreate --no-build postgres qarax
	wait_for_http "${API_URL}/" 60
}

ensure_libvirt_network() {
	if ! virsh net-info default >/dev/null 2>&1; then
		echo "libvirt network 'default' not found. Configure libvirt networking first." >&2
		exit 1
	fi
	virsh net-start default >/dev/null 2>&1 || true
	virsh net-autostart default >/dev/null 2>&1 || true
}

cleanup_vm() {
	if virsh dominfo "${VM_NAME}" >/dev/null 2>&1; then
		virsh destroy "${VM_NAME}" >/dev/null 2>&1 || true
		virsh undefine "${VM_NAME}" --nvram >/dev/null 2>&1 || virsh undefine "${VM_NAME}" >/dev/null 2>&1 || true
	fi
	rm -f "${VM_DISK}"
}

build_test_host_image() {
	local build_dir="${STATE_DIR}/build"
	mkdir -p "${build_dir}"

	# Copy real binaries into the build context
	local node_bin="${REPO_ROOT}/target/x86_64-unknown-linux-musl/release/qarax-node"
	local init_bin="${REPO_ROOT}/target/x86_64-unknown-linux-musl/release/qarax-init"
	if [[ ! -f "${node_bin}" ]]; then
		echo "qarax-node binary not found at ${node_bin}." >&2
		echo "Build it first: cargo build --release -p qarax-node" >&2
		exit 1
	fi
	if [[ ! -f "${init_bin}" ]]; then
		echo "qarax-init binary not found at ${init_bin}." >&2
		echo "Build it first: cargo build --release -p qarax-init" >&2
		exit 1
	fi
	cp "${node_bin}" "${build_dir}/qarax-node"
	cp "${init_bin}" "${build_dir}/qarax-init"

	# Copy systemd service files
	cp "${REPO_ROOT}/deployments/qarax-node.service" "${build_dir}/qarax-node.service"
	cp "${REPO_ROOT}/deployments/overlaybd-tcmu.service" "${build_dir}/overlaybd-tcmu.service"

	local overlaybd_ver="${OVERLAYBD_VERSION:-1.0.16}"
	local overlaybd_rpm_suffix="${OVERLAYBD_RPM_SUFFIX:-20250818.4601fb2}"
	local overlaybd_rpm_dist="${OVERLAYBD_RPM_DIST:-azurelinux.3.0}"
	local aci_ver="${ACI_VERSION:-1.4.1}"
	local aci_rpm_suffix="${ACI_RPM_SUFFIX:-20260114025326.c6ba42c}"

	cat >"${build_dir}/Containerfile" <<DOCKERFILE
FROM ${VM_BOOTC_IMAGE}

ARG OVERLAYBD_VERSION=${overlaybd_ver}
ARG OVERLAYBD_RPM_SUFFIX=${overlaybd_rpm_suffix}
ARG OVERLAYBD_RPM_DIST=${overlaybd_rpm_dist}
ARG ACI_VERSION=${aci_ver}
ARG ACI_RPM_SUFFIX=${aci_rpm_suffix}

RUN dnf install -y \
    qemu-guest-agent \
    e2fsprogs \
    cpio \
    virtiofsd \
    curl \
    && dnf clean all \
    && ln -sf /usr/libexec/virtiofsd /usr/local/bin/virtiofsd

RUN useradd -m -G wheel -s /bin/bash ${SSH_USER} && \
    echo '${SSH_USER}:${SSH_PASSWORD}' | chpasswd && \
    echo '${SSH_USER} ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/${SSH_USER}

RUN mkdir -p /etc/ssh/sshd_config.d && \
    echo 'PasswordAuthentication yes' > /etc/ssh/sshd_config.d/01-qarax-test.conf

# Allow pulling from insecure (HTTP) registries for local testing
RUN mkdir -p /etc/containers/registries.conf.d && \
    printf '%s\n' \
      '[[registry]]' \
      'location = "192.168.122.1:5001"' \
      'insecure = true' \
      > /etc/containers/registries.conf.d/01-local-test.conf

# Install overlaybd-tcmu
RUN curl -fsSL \
    "https://github.com/containerd/overlaybd/releases/download/v\${OVERLAYBD_VERSION}/overlaybd-\${OVERLAYBD_VERSION}-\${OVERLAYBD_RPM_SUFFIX}.\${OVERLAYBD_RPM_DIST}.x86_64.rpm" \
    -o /tmp/overlaybd.rpm && \
    cd / && rpm2cpio /tmp/overlaybd.rpm | cpio -idm --no-absolute-filenames 2>/dev/null && \
    rm /tmp/overlaybd.rpm

# Patch overlaybd config to use writable paths under /var (bootc makes /opt read-only).
RUN sed -i \
    -e 's|/opt/overlaybd/registry_cache|/var/lib/overlaybd/registry_cache|g' \
    -e 's|/opt/overlaybd/gzip_cache|/var/lib/overlaybd/gzip_cache|g' \
    -e 's|/opt/overlaybd/cred.json|/etc/overlaybd/cred.json|g' \
    /etc/overlaybd/overlaybd.json && \
    cp /opt/overlaybd/cred.json /etc/overlaybd/cred.json 2>/dev/null || \
    echo '{"auths":{}}' > /etc/overlaybd/cred.json

# Install the overlaybd-snapshotter convertor
RUN curl -fsSL \
    "https://github.com/containerd/accelerated-container-image/releases/download/v\${ACI_VERSION}/overlaybd-snapshotter-\${ACI_VERSION}-\${ACI_RPM_SUFFIX}.x86_64.rpm" \
    -o /tmp/aci.rpm && \
    cd / && rpm2cpio /tmp/aci.rpm | cpio -idm --no-absolute-filenames 2>/dev/null && \
    rm /tmp/aci.rpm

# Install Cloud Hypervisor (static binary)
RUN curl -fsSL \
    "https://github.com/cloud-hypervisor/cloud-hypervisor/releases/download/${CLOUD_HYPERVISOR_VERSION}/cloud-hypervisor-static" \
    -o /usr/local/bin/cloud-hypervisor && \
    chmod +x /usr/local/bin/cloud-hypervisor && \
    cloud-hypervisor --version

# Download Cloud Hypervisor guest kernel (used by qarax-node to boot VMs)
RUN mkdir -p /var/lib/qarax/images && \
    curl -fsSL \
    "https://github.com/cloud-hypervisor/linux/releases/download/ch-release-v6.16.9-20251112/vmlinux-x86_64" \
    -o /var/lib/qarax/images/vmlinux && \
    chmod 644 /var/lib/qarax/images/vmlinux

# Load TCMU kernel modules at boot
RUN printf '%s\n' target_core_user tcm_loop uio \
    >> /etc/modules-load.d/overlaybd.conf

# Cache-buster: changes when qarax-node binary changes
ARG CACHE_BUST=none

# Copy qarax binaries
COPY qarax-node /usr/local/bin/qarax-node
COPY qarax-init /usr/local/bin/qarax-init
RUN chmod +x /usr/local/bin/qarax-node /usr/local/bin/qarax-init

# Create runtime directories via tmpfiles.d (bootc preserves /var across
# image switches, so RUN mkdir only works on first boot — tmpfiles.d
# ensures directories exist on every boot).
RUN printf '%s\n' \
    'd /var/lib/qarax      0755 root root -' \
    'd /var/lib/qarax/vms  0755 root root -' \
    'd /var/lib/qarax/images 0755 root root -' \
    'd /var/lib/qarax/disks 0755 root root -' \
    'd /var/lib/qarax/overlaybd 0755 root root -' \
    'd /var/lib/overlaybd 0755 root root -' \
    'd /var/lib/overlaybd/registry_cache 0755 root root -' \
    'd /var/lib/overlaybd/gzip_cache 0755 root root -' \
    > /etc/tmpfiles.d/qarax.conf && \
    mkdir -p /etc/qarax-node

COPY overlaybd-tcmu.service /etc/systemd/system/
COPY qarax-node.service /etc/systemd/system/
RUN systemctl enable overlaybd-tcmu.service qarax-node.service sshd.service qemu-guest-agent.service
DOCKERFILE

	echo "Building test host container image..."
	# Use binary checksum as a cache-buster so COPY layers are invalidated when
	# the qarax-node binary changes (podman's layer cache may not detect it).
	local node_hash
	node_hash="$(sha256sum "${build_dir}/qarax-node" | cut -d' ' -f1)"
	sudo podman build \
		--build-arg "CACHE_BUST=${node_hash}" \
		-t "${TEST_HOST_TAG}" -f "${build_dir}/Containerfile" "${build_dir}"
	# Re-tagging leaves the previous image dangling (untagged). Prune them now
	# so they don't accumulate across repeated builds. Layer cache is preserved.
	sudo podman image prune -f
	echo "Pushing test host image to local registry ${LOCAL_REGISTRY}..."
	sudo podman push "${TEST_HOST_TAG}" "${DEPLOY_IMAGE}" --tls-verify=false
}

build_disk_image() {
	if [[ -f "${BASE_IMAGE}" ]]; then
		echo "Using cached disk image: ${BASE_IMAGE}"
		# Still need to ensure the test image is in the local registry for bootc switch
		ensure_test_image_in_local_registry
		return 0
	fi

	build_test_host_image

	local bib_output="${STATE_DIR}/bib-output"
	mkdir -p "${bib_output}"

	echo "Building disk image with bootc-image-builder..."
	sudo podman run --rm --privileged \
		--security-opt label=type:unconfined_t \
		-v /var/lib/containers/storage:/var/lib/containers/storage \
		-v "${bib_output}:/output" \
		quay.io/centos-bootc/bootc-image-builder:latest \
		--type qcow2 \
		--local \
		"${TEST_HOST_TAG}"

	sudo mv "${bib_output}/qcow2/disk.qcow2" "${BASE_IMAGE}"
	sudo chown "$(id -u):$(id -g)" "${BASE_IMAGE}"
	sudo rm -rf "${bib_output}"
	echo "Disk image ready: ${BASE_IMAGE}"
}

ensure_test_image_in_local_registry() {
	# Always rebuild to pick up binary changes (podman layer cache handles
	# the COPY layer efficiently — only re-layers when the file content differs).
	build_test_host_image
}

create_vm() {
	mkdir -p "${STATE_DIR}"
	build_disk_image

	qemu-img create -f qcow2 -F qcow2 -b "${BASE_IMAGE}" "${VM_DISK}" "${VM_DISK_SIZE}" >/dev/null

	virt-install \
		--name "${VM_NAME}" \
		--memory "${VM_MEMORY_MB}" \
		--vcpus "${VM_VCPUS}" \
		--cpu host-passthrough \
		--import \
		--disk "path=${VM_DISK},format=qcow2,bus=virtio" \
		--network "network=default,model=virtio" \
		--os-variant centos-stream10 \
		--graphics none \
		--noautoconsole \
		--rng /dev/urandom
}

wait_for_vm_ip() {
	local timeout_s="${1:-240}"
	local start_ts ip
	start_ts="$(date +%s)"

	while true; do
		ip="$(
			virsh domifaddr "${VM_NAME}" --source lease 2>/dev/null |
				awk '/ipv4/ {print $4}' |
				cut -d/ -f1 |
				head -n1
		)"
		if [[ -n "${ip}" ]]; then
			echo "${ip}"
			return 0
		fi

		if (("$(date +%s)" - start_ts >= timeout_s)); then
			echo "Timed out waiting for VM IP address" >&2
			return 1
		fi
		sleep 2
	done
}

wait_for_ssh_auth() {
	local host="$1"
	local timeout_s="${2:-90}"
	local start_ts
	start_ts="$(date +%s)"
	while true; do
		if sshpass -p "${SSH_PASSWORD}" ssh \
			-o StrictHostKeyChecking=no \
			-o UserKnownHostsFile=/dev/null \
			-o ConnectTimeout=5 \
			-o BatchMode=no \
			-p "${SSH_PORT}" \
			"${SSH_USER}@${host}" true >/dev/null 2>&1; then
			return 0
		fi
		if (("$(date +%s)" - start_ts >= timeout_s)); then
			echo "Timed out waiting for SSH auth at ${host}" >&2
			echo "" >&2
			echo "--- Diagnostic: one SSH attempt with verbose output ---" >&2
			sshpass -p "${SSH_PASSWORD}" ssh \
				-vv \
				-o StrictHostKeyChecking=no \
				-o UserKnownHostsFile=/dev/null \
				-o ConnectTimeout=5 \
				-p "${SSH_PORT}" \
				"${SSH_USER}@${host}" true 2>&1 || true
			echo "--- End diagnostic ---" >&2
			echo "" >&2
			echo "Retry manually: sshpass -p '${SSH_PASSWORD}' ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -p ${SSH_PORT} ${SSH_USER}@${host}" >&2
			return 1
		fi
		sleep 2
	done
}

wait_for_tcp() {
	local host="$1"
	local port="$2"
	local timeout_s="$3"
	local start_ts
	start_ts="$(date +%s)"
	while true; do
		if nc -z "${host}" "${port}" >/dev/null 2>&1; then
			return 0
		fi
		if (("$(date +%s)" - start_ts >= timeout_s)); then
			echo "Timed out waiting for ${host}:${port}" >&2
			return 1
		fi
		sleep 2
	done
}

lookup_host_id_by_address() {
	local address="$1"
	curl -sS "${API_URL}/hosts" | python3 -c "
import json, sys
address = '$address'
for host in json.load(sys.stdin):
    if host.get('address') == address:
        print(host['id'])
        break
"
}

lookup_host_status() {
	local host_id="$1"
	curl -sS "${API_URL}/hosts" | python3 -c "
import json, sys
host_id = '$host_id'
for host in json.load(sys.stdin):
    if host.get('id') == host_id:
        print(host.get('status', ''))
        break
"
}

register_host() {
	local vm_ip="$1"
	local host_name="libvirt-${VM_NAME}-$(date +%s)"
	local payload
	payload="$(jq -n \
		--arg name "${host_name}" \
		--arg address "${vm_ip}" \
		--arg host_user "${SSH_USER}" \
		--arg password "" \
		--argjson port "${NODE_PORT}" \
		'{name:$name,address:$address,port:$port,host_user:$host_user,password:$password}')"

	local body_file code host_id
	body_file="$(mktemp)"
	code="$(
		curl -sS -o "${body_file}" -w "%{http_code}" \
			-X POST "${API_URL}/hosts" \
			-H "Content-Type: application/json" \
			-d "${payload}"
	)"

	if [[ "${code}" != "201" ]]; then
		echo "Failed to register host (HTTP ${code}):" >&2
		cat "${body_file}" >&2
		rm -f "${body_file}"
		exit 1
	fi

	host_id="$(tr -d '"' <"${body_file}")"
	rm -f "${body_file}"
	echo "${host_id}"
}

trigger_deploy() {
	local host_id="$1"
	local payload body_file code
	payload="$(jq -n \
		--arg image "${DEPLOY_IMAGE}" \
		--arg ssh_user "${SSH_USER}" \
		--arg ssh_password "${SSH_PASSWORD}" \
		--argjson ssh_port "${SSH_PORT}" \
		'{
      image:$image,
      ssh_port:$ssh_port,
      ssh_user:$ssh_user,
      ssh_password:$ssh_password,
      install_bootc:false,
      reboot:true
    }')"

	body_file="$(mktemp)"
	code="$(
		curl -sS -o "${body_file}" -w "%{http_code}" \
			-X POST "${API_URL}/hosts/${host_id}/deploy" \
			-H "Content-Type: application/json" \
			-d "${payload}"
	)"

	if [[ "${code}" != "202" ]]; then
		echo "Deploy request failed (HTTP ${code}):" >&2
		cat "${body_file}" >&2
		rm -f "${body_file}"
		exit 1
	fi
	rm -f "${body_file}"
}

wait_for_deploy_success() {
	local host_id="$1"
	local vm_ip="$2"
	local timeout_s="${3:-420}"
	local start_ts status saw_installing
	start_ts="$(date +%s)"
	saw_installing=0

	while true; do
		status="$(lookup_host_status "${host_id}")"
		case "${status}" in
		installing)
			saw_installing=1
			;;
		up)
			if [[ "${saw_installing}" -eq 1 ]]; then
				return 0
			fi
			;;
		installation_failed)
			echo "Host deployment failed (status=installation_failed)." >&2
			echo "" >&2
			echo "  Logs: docker logs e2e-qarax-1"
			if [[ -n "${vm_ip}" ]]; then
				echo "  sshpass -p '${SSH_PASSWORD}' ssh -o StrictHostKeyChecking=no ${SSH_USER}@${vm_ip}" >&2
				echo "  Re-run with --keep-vm to preserve the VM after failure" >&2
			fi
			return 1
			;;
		"")
			echo "Host ${host_id} not found while polling status." >&2
			return 1
			;;
		esac

		if (("$(date +%s)" - start_ts >= timeout_s)); then
			echo "Timed out waiting for host deployment to finish. Last status=${status}" >&2
			return 1
		fi
		sleep 5
	done
}

run_connectivity_probe_from_qarax_container() {
	# With network_mode: host the container shares the host's network namespace,
	# so a host-side nc check is equivalent to checking from inside the container.
	local vm_ip="$1"
	if ! nc -zw3 "${vm_ip}" "${SSH_PORT}" >/dev/null 2>&1; then
		echo "Cannot reach ${vm_ip}:${SSH_PORT} - libvirt VM not reachable." >&2
		exit 1
	fi
}

main() {
	require_cmd virsh
	require_cmd virt-install
	require_cmd qemu-img
	require_cmd podman
	require_cmd curl
	require_cmd jq
	require_cmd nc
	require_cmd python3
	require_cmd sshpass

	ensure_libvirt_network

	if [[ "${CLEANUP_ONLY}" -eq 1 ]]; then
		cleanup_vm
		echo "Cleanup complete for VM ${VM_NAME}."
		exit 0
	fi

	ensure_stack

	cleanup_vm
	create_vm

	if [[ "${KEEP_VM}" -ne 1 ]]; then
		trap cleanup_vm EXIT
	fi

	local vm_ip host_id
	vm_ip="$(wait_for_vm_ip 120)"
	echo "VM IP: ${vm_ip}"

	wait_for_tcp "${vm_ip}" "${SSH_PORT}" 120
	wait_for_tcp "${vm_ip}" "${NODE_PORT}" 120
	run_connectivity_probe_from_qarax_container "${vm_ip}"
	echo "Waiting for SSH auth to succeed on ${vm_ip}..."
	wait_for_ssh_auth "${vm_ip}" 90

	host_id="$(register_host "${vm_ip}")"
	echo "Registered host id: ${host_id}"

	trigger_deploy "${host_id}"
	echo "Deploy triggered. Waiting for host to become UP..."

	wait_for_deploy_success "${host_id}" "${vm_ip}" 420

	echo ""
	echo "SUCCESS: deploy flow completed against real libvirt VM."
	echo "Host ${host_id} is UP."
	echo "VM ${VM_NAME} (${vm_ip})"
	if [[ "${KEEP_VM}" -eq 1 ]]; then
		echo "VM was kept because --keep-vm was set."
	fi
}

main "$@"
