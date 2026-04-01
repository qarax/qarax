#!/bin/sh
# Entrypoint for qarax-node container (E2E testing).
# Loads kernel modules, creates UIO device nodes, starts overlaybd-tcmu,
# then runs qarax-node.
set -e

# Kernel modules
modprobe target_core_user 2>/dev/null || true
modprobe tcm_loop 2>/dev/null || true
modprobe uio 2>/dev/null || true
modprobe vhost_vsock 2>/dev/null || true

sync_uio_nodes() {
	for sys_uio in /sys/class/uio/uio*; do
		[ -e "$sys_uio" ] || continue
		node="/dev/${sys_uio##*/}"
		[ -e "$node" ] && continue
		if IFS=: read -r major minor <"$sys_uio/dev"; then
			echo "Creating UIO device node ${node} (${major}:${minor})"
			mknod "$node" c "$major" "$minor"
		fi
	done
}

UIO_MAJOR=$(awk '/[[:space:]]uio$/{print $1}' /proc/devices 2>/dev/null | head -1)
if [ -n "$UIO_MAJOR" ]; then
	echo "Pre-creating common UIO device nodes (major=${UIO_MAJOR})"
	for i in 0 1 2 3 4 5 6 7; do
		[ -e "/dev/uio${i}" ] || mknod "/dev/uio${i}" c "${UIO_MAJOR}" "${i}"
	done
else
	echo "WARNING: could not determine UIO major; overlaybd-tcmu may fail"
fi

# Keep /dev/uioN in sync with the real sysfs devices. The container has no udev,
# so overlaybd-tcmu otherwise sees missing device nodes when target_core_user
# allocates a fresh UIO device during TCMU enable.
sync_uio_nodes
(
	while true; do
		sync_uio_nodes
		sleep 0.1
	done
) &

# overlaybd-tcmu
if [ "${START_OVERLAYBD_TCMU:-1}" = "1" ] && [ -x /opt/overlaybd/bin/overlaybd-tcmu ]; then
	echo "Starting overlaybd-tcmu..."
	/opt/overlaybd/bin/overlaybd-tcmu &
	sleep 1
	echo "overlaybd-tcmu started"
else
	echo "overlaybd-tcmu disabled for this node"
fi

exec /usr/local/bin/qarax-node --port 50051 "$@"
