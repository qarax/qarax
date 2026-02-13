#!/bin/sh
# Minimal init script for testing Cloud Hypervisor VMs
# This just prints a message and halts - useful for verifying VM boot works

mount -t proc none /proc
mount -t sysfs none /sys
mount -t devtmpfs none /dev

echo "=========================================="
echo "  qarax test VM booted successfully!"
echo "=========================================="
echo ""
echo "Kernel: $(uname -r)"
echo "CPU: $(grep -c processor /proc/cpuinfo) vCPUs"
echo "Memory: $(grep MemTotal /proc/meminfo)"
echo ""

# Keep running for a moment so tests can verify state
sleep 5

echo "Test VM shutting down..."
poweroff -f
