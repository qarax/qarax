#!/bin/sh
# Entrypoint for qarax-node container (E2E testing).
# Loads kernel modules, creates UIO device nodes, starts overlaybd-tcmu,
# then runs qarax-node.
set -e

# Kernel modules──────────
modprobe target_core_user 2>/dev/null || true
modprobe tcm_loop 2>/dev/null || true

# UIO device nodes──────────
UIO_MAJOR=$(awk '/[[:space:]]uio$/{print $1}' /proc/devices 2>/dev/null | head -1)
if [ -n "$UIO_MAJOR" ]; then
	echo "Creating UIO device nodes (major=${UIO_MAJOR})"
	for i in 0 1 2 3 4 5 6 7; do
		[ -e "/dev/uio${i}" ] || mknod "/dev/uio${i}" c "${UIO_MAJOR}" "${i}"
	done
else
	echo "WARNING: could not determine UIO major; overlaybd-tcmu may fail"
fi

# overlaybd-tcmu──────────
if [ -x /opt/overlaybd/bin/overlaybd-tcmu ]; then
	echo "Starting overlaybd-tcmu..."
	/opt/overlaybd/bin/overlaybd-tcmu &
	sleep 1
	echo "overlaybd-tcmu started"
else
	echo "overlaybd-tcmu not found, OverlayBD disabled"
fi

exec /usr/local/bin/qarax-node --port 50051 "$@"
