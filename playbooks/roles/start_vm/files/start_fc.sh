#!/bin/bash -e

sudo modprobe kvm_intel

sudo sysctl -w net.ipv4.conf.all.forwarding=1

SB_ID="0"
TAP_DEV="fc-${SB_ID}-tap0"

# Setup TAP device that uses proxy ARP
CIDR="/30"
NETMASK=255.255.255.252
FC_IP="$(printf '169.254.%s.%s' $(((4 * SB_ID + 1) / 256)) $(((4 * SB_ID + 1) % 256)))"
TAP_IP="$(printf '169.254.%s.%s' $(((4 * SB_ID + 2) / 256)) $(((4 * SB_ID + 2) % 256)))"
FC_MAC="$(printf '02:FC:00:00:%02X:%02X' $((SB_ID / 256)) $((SB_ID % 256)))"
sudo ip link del "$TAP_DEV" 2> /dev/null || true
sudo ip tuntap add dev "$TAP_DEV" mode tap
sudo sysctl -w net.ipv4.conf.${TAP_DEV}.proxy_arp=1 > /dev/null
sudo sysctl -w net.ipv6.conf.${TAP_DEV}.disable_ipv6=1 > /dev/null
sudo ip addr add "${TAP_IP}${CIDR}" dev "${TAP_DEV}"
sudo ip link set dev "$TAP_DEV" up

./firectl --kernel=./vmlinux  --root-drive=./xenial.rootfs.ext4 --kernel-opts="console=ttyS0 noapic reboot=k panic=1 pci=off nomodules rw ip=${FC_IP}::${TAP_IP}:${NETMASK}::eth0:off" --tap-device="${TAP_DEV}/${FC_MAC}" --firecracker-binary=./firecracker -m=128
