#!/bin/sh
set -eu

SHARED_DIR="${SHARED_DIRECTORY:-/nfs-export}"
mkdir -p "$SHARED_DIR"

echo "${SHARED_DIR} *(rw,sync,no_subtree_check,no_root_squash)" >/etc/exports

RPCBIND_PID=""
MOUNTD_PID=""
REFRESH_PID=""

cleanup() {
	trap - EXIT INT TERM

	echo "Shutting down NFS server..."
	exportfs -au 2>/dev/null || true
	exportfs -f 2>/dev/null || true
	rpc.nfsd 0 2>/dev/null || true

	if [ -n "${REFRESH_PID}" ]; then
		kill "${REFRESH_PID}" 2>/dev/null || true
		wait "${REFRESH_PID}" 2>/dev/null || true
	fi

	if [ -n "${MOUNTD_PID}" ]; then
		kill "${MOUNTD_PID}" 2>/dev/null || true
		wait "${MOUNTD_PID}" 2>/dev/null || true
	fi

	if [ -n "${RPCBIND_PID}" ]; then
		kill "${RPCBIND_PID}" 2>/dev/null || true
		wait "${RPCBIND_PID}" 2>/dev/null || true
	fi

	umount /proc/fs/nfsd 2>/dev/null || true
}

trap cleanup EXIT INT TERM

# Mount the nfsd kernel filesystem — requires --privileged
mount -t nfsd nfsd /proc/fs/nfsd 2>/dev/null || true

rpcbind -f -w &
RPCBIND_PID=$!

# Wait for rpcbind to be ready before starting other RPC services
until rpcinfo -p localhost >/dev/null 2>&1; do
	sleep 0.5
done

rpc.nfsd 8
rpc.mountd --no-udp -F &
MOUNTD_PID=$!
exportfs -ra

echo "NFS server ready: $(cat /etc/exports)"

# Stay alive and keep exports current
(
	while true; do
		sleep 30
		exportfs -ra 2>/dev/null || true
	done
) &
REFRESH_PID=$!

wait "${MOUNTD_PID}"
