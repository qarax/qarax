#!/bin/sh
# Entrypoint for the QEMU bootc-vm container.
#
# Starts a socat proxy that forwards the QEMU SLIRP gateway port 5000 to the
# Docker Compose registry (registry:5000).  This allows the guest VM to pull
# images for bootc switch using the URL registry:5000/<image>.
#
# Then boots the overlay qcow2 disk under QEMU/KVM with port-forwarding so
# that the guest's SSH (22) and qarax-node gRPC (50051) are accessible on the
# container's network interface.
set -e

DISK=/disk/bootc-vm-overlay.qcow2

if [ ! -f "$DISK" ]; then
	echo "ERROR: disk image not found at $DISK" >&2
	exit 1
fi

# Proxy the local registry into the VM via the SLIRP gateway.
# From inside the guest: registry:5000 → 10.0.2.2:5000 → socat → registry:5000
socat TCP-LISTEN:5000,fork,reuseaddr TCP:registry:5000 &

echo "Starting QEMU VM from $DISK ..."
exec qemu-system-x86_64 \
	-enable-kvm \
	-cpu host \
	-m 2048 \
	-smp 2 \
	-drive "file=${DISK},format=qcow2,if=virtio" \
	-netdev "user,id=net0,hostfwd=tcp:0.0.0.0:22-:22,hostfwd=tcp:0.0.0.0:50051-:50051" \
	-device virtio-net-pci,netdev=net0 \
	-nographic
