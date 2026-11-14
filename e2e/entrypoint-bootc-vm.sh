#!/bin/sh
# Boot the bootc node disk under Cloud Hypervisor and make the guest reachable
# through this container (see Dockerfile.bootc-vm):
#   guest 10.0.2.15 <-> tap vmtap0 (10.0.2.2, DHCP from dnsmasq) <-> NAT <-> compose network
#   container:22 and :50051 are forwarded to the guest
#   10.0.2.2:5000 inside the guest is the compose registry (socat)
set -eu

DISK=/disk/bootc-vm-overlay.qcow2
TAP=vmtap0
GATEWAY=10.0.2.2
GUEST_IP=10.0.2.15
# Fixed, so a reboot (bootc switch) gets the same DHCP lease.
GUEST_MAC=52:54:00:12:34:56

[ -f "$DISK" ] || {
	echo "ERROR: disk image not found at $DISK" >&2
	exit 1
}

uplink=$(ip -o route show default | awk '{print $5; exit}')

ip tuntap add dev "$TAP" mode tap
ip addr add "${GATEWAY}/24" dev "$TAP"
ip link set "$TAP" up

echo 1 >/proc/sys/net/ipv4/ip_forward
# Guest traffic out to the compose network and the internet.
iptables -t nat -A POSTROUTING -s 10.0.2.0/24 -o "$uplink" -j MASQUERADE
# The container's SSH and gRPC ports are the guest's.
for port in 22 50051; do
	iptables -t nat -A PREROUTING -i "$uplink" -p tcp --dport "$port" \
		-j DNAT --to-destination "${GUEST_IP}:${port}"
done

dnsmasq --interface="$TAP" --bind-interfaces \
	--dhcp-range="${GUEST_IP},${GUEST_IP},255.255.255.0,12h" \
	--dhcp-host="${GUEST_MAC},${GUEST_IP}" \
	--dhcp-option=option:router,"$GATEWAY" \
	--dhcp-option=option:dns-server,"$GATEWAY"

socat "TCP-LISTEN:5000,bind=${GATEWAY},fork,reuseaddr" TCP:registry:5000 &

echo "Booting ${DISK} under Cloud Hypervisor ($(cloud-hypervisor --version))"
exec cloud-hypervisor \
	--firmware /usr/share/cloud-hypervisor/CLOUDHV.fd \
	--cpus "boot=${VM_CPUS:-2},nested=on" \
	--memory "size=${VM_MEMORY_MIB:-2048}M" \
	--disk "path=${DISK},image_type=qcow2,backing_files=on" \
	--net "tap=${TAP},mac=${GUEST_MAC}" \
	--rng src=/dev/urandom \
	--serial tty \
	--console off
