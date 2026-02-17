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
# Use only /proc and shell (no uname/grep/cut - minimal BusyBox may not have them)
read -r ver_line < /proc/version 2>/dev/null || ver_line=""
set -- $ver_line 2>/dev/null
echo "Kernel: ${3:-unknown}"
cpu_count=0
while read -r line; do
  case "$line" in processor*) cpu_count=$((cpu_count+1)) ;; esac
done < /proc/cpuinfo
echo "CPU: ${cpu_count} vCPUs"
while read -r line; do
  case "$line" in MemTotal*) echo "Memory: $line"; break ;; esac
done < /proc/meminfo
echo ""

# Keep running for a moment so tests can verify state
sleep 5

echo "Test VM shutting down..."
poweroff -f
