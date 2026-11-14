#!/usr/bin/env bash
# Build the bootc test node and the disks the bootc-vm compose service boots:
#
#   localhost:5001/qarax-vmm-host:e2e     the appliance (deployments/Containerfile.qarax-vmm)
#   localhost:5001/qarax-node-bootc:base  plus the test layer (e2e/Containerfile.bootc-node)
#   e2e/bootc-vm-base.qcow2               `bootc install to-disk` of that image,
#                                         rebuilt only when the image changes
#   e2e/bootc-vm-overlay.qcow2            a fresh copy-on-write layer over it
#
# Needs the release binaries, the stack's registry on localhost:5001, docker
# and qemu-img. The install runs in a privileged container (it uses a loop
# device), so neither sudo nor podman is needed.
#
# Honors DOCKER_BUILDX / DOCKER_BUILD_CACHE_FROM / DOCKER_BUILD_CACHE_TO like
# run_e2e_tests.sh, so CI reuses its BuildKit cache.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
E2E_DIR="${REPO_ROOT}/e2e"
REGISTRY="localhost:5001"
APPLIANCE_IMAGE="${REGISTRY}/qarax-vmm-host:e2e"
NODE_IMAGE="${REGISTRY}/qarax-node-bootc:base"
BASE_DISK="${E2E_DIR}/bootc-vm-base.qcow2"
OVERLAY_DISK="${E2E_DIR}/bootc-vm-overlay.qcow2"
DISK_SIZE="${BOOTC_DISK_SIZE:-12G}"

build_image() {
	local scope="$1"
	shift
	if [[ -n "${DOCKER_BUILDX:-}${DOCKER_BUILD_CACHE_FROM:-}${DOCKER_BUILD_CACHE_TO:-}" ]]; then
		local cmd=(docker buildx build --load)
		[[ -z "${DOCKER_BUILD_CACHE_FROM:-}" ]] || cmd+=(--cache-from "${DOCKER_BUILD_CACHE_FROM},scope=${scope}")
		[[ -z "${DOCKER_BUILD_CACHE_TO:-}" ]] || cmd+=(--cache-to "${DOCKER_BUILD_CACHE_TO},scope=${scope}")
		"${cmd[@]}" "$@"
	else
		docker build "$@"
	fi
}

echo "Building the appliance image (${APPLIANCE_IMAGE})..."
build_image e2e-appliance \
	-f "${REPO_ROOT}/deployments/Containerfile.qarax-vmm" \
	--build-arg "CLOUD_HYPERVISOR_VERSION=$(tr -d '\n' <"${REPO_ROOT}/versions/cloud-hypervisor-version")" \
	--build-arg "FIRECRACKER_VERSION=$(tr -d '\n' <"${REPO_ROOT}/versions/firecracker-version")" \
	--build-arg "NODE_ARTIFACTS_SHA=$(sha256sum "${REPO_ROOT}/versions/node-artifacts.env" | cut -d' ' -f1)" \
	--build-arg QARAX_VERSION=e2e \
	-t "$APPLIANCE_IMAGE" \
	"$REPO_ROOT"

# The test layer builds FROM the appliance image loaded above, which only the
# daemon's own builder can see: CI's docker-container builder (buildx) would
# try to pull it, and its localhost isn't this host's registry.
echo "Building the test node image (${NODE_IMAGE})..."
docker buildx build --builder default \
	-f "${E2E_DIR}/Containerfile.bootc-node" \
	--build-arg "BASE_IMAGE=${APPLIANCE_IMAGE}" \
	-t "$NODE_IMAGE" \
	"$E2E_DIR"
docker push -q "$NODE_IMAGE"

image_id=$(docker image inspect --format '{{.Id}}' "$NODE_IMAGE")
if [[ -f "$BASE_DISK" && "$(cat "${BASE_DISK}.image-id" 2>/dev/null)" == "$image_id" ]]; then
	echo "Reusing ${BASE_DISK} (image unchanged)"
else
	echo "Installing ${NODE_IMAGE} to ${BASE_DISK}..."
	raw="${E2E_DIR}/.bootc-vm-install.raw"
	registries="${E2E_DIR}/.bootc-vm-install-registries.conf"
	trap 'rm -f "$raw" "$registries"' EXIT
	rm -f "$raw"
	truncate -s "$DISK_SIZE" "$raw"
	# The installer pulls the image itself, over plain HTTP from the local registry.
	printf '[[registry]]\nlocation = "%s"\ninsecure = true\n' "$REGISTRY" >"$registries"
	docker run --rm --privileged --pid=host --network host \
		-v /dev:/dev \
		-v "${E2E_DIR}:/output" \
		-v "${registries}:/etc/containers/registries.conf.d/99-local.conf:ro" \
		"$NODE_IMAGE" \
		bootc install to-disk --via-loopback --generic-image --wipe \
		--source-imgref "registry:${NODE_IMAGE}" "/output/$(basename "$raw")"
	qemu-img convert -O qcow2 "$raw" "$BASE_DISK"
	echo "$image_id" >"${BASE_DISK}.image-id"
fi

# A fresh layer per run keeps the base pristine. The backing file is named
# relative to the overlay, so the path holds inside the container (/disk).
rm -f "$OVERLAY_DISK"
qemu-img create -q -f qcow2 -F qcow2 -b "$(basename "$BASE_DISK")" "$OVERLAY_DISK"
echo "bootc node disk ready: ${OVERLAY_DISK}"
