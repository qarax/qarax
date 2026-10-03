#!/usr/bin/env bash
# LIO iSCSI target bootstrap for the BLOCK storage demo.
#
# Exports one fileio-backed LUN (default: 1 GiB) over iSCSI with a wide-open
# ACL (demo_mode enabled). IQN and LUN size are configurable via env vars:
#
#   TARGET_IQN   iSCSI target IQN (default: iqn.2024-01.io.qarax:demo)
#   LUN_SIZE     backing file size in bytes (default: 1073741824 = 1 GiB)
#   LUN_PATH     backing file path (default: /var/lib/qarax-block/lun0.img)
#
# LIO state lives in the host kernel (configfs), not in the container, so it
# outlives the container unless removed. The script removes this demo's target
# and backstore on start (stale state from an earlier run) and on SIGTERM.

set -euo pipefail

TARGET_IQN="${TARGET_IQN:-iqn.2024-01.io.qarax:demo}"
LUN_PATH="${LUN_PATH:-/var/lib/qarax-block/lun0.img}"
LUN_SIZE="${LUN_SIZE:-1073741824}"
BACKSTORE="demo_lun0"

# Only ever delete this demo's objects. Never use `clearconfig`: the host's LIO
# instance is shared with qarax-node (OverlayBD TCMU devices).
remove_target() {
	targetcli /iscsi delete "${TARGET_IQN}" >/dev/null 2>&1 || true
	targetcli /backstores/fileio delete "${BACKSTORE}" >/dev/null 2>&1 || true
}

shutdown() {
	echo "==> Removing LIO target ${TARGET_IQN}"
	remove_target
	exit 0
}

echo "==> BLOCK demo target: ${TARGET_IQN}  path=${LUN_PATH}  size=${LUN_SIZE}"

modprobe target_core_mod 2>/dev/null || true
modprobe target_core_file 2>/dev/null || true
modprobe iscsi_target_mod 2>/dev/null || true

mkdir -p "$(dirname "${LUN_PATH}")"

if [[ ! -f "${LUN_PATH}" ]]; then
	echo "==> Allocating backing file"
	truncate -s "${LUN_SIZE}" "${LUN_PATH}"
fi

remove_target
trap shutdown TERM INT

# targetcli queries tcmu-runner over the D-Bus system bus whenever the host
# kernel has TCMU user backstores (qarax-node's OverlayBD creates them), and
# crashes if there is no bus at all. A bare bus lets that lookup fail cleanly.
mkdir -p /run/dbus
dbus-daemon --system --fork

echo "==> Configuring LIO via targetcli"
targetcli <<EOF
/backstores/fileio create ${BACKSTORE} ${LUN_PATH} ${LUN_SIZE}
/iscsi create ${TARGET_IQN}
/iscsi/${TARGET_IQN}/tpg1/luns create /backstores/fileio/${BACKSTORE} 0
/iscsi/${TARGET_IQN}/tpg1 set attribute authentication=0 demo_mode_write_protect=0 generate_node_acls=1 cache_dynamic_acls=1
/iscsi/${TARGET_IQN}/tpg1/portals create 0.0.0.0 3260
exit
EOF

echo "==> Target ready on port 3260"

# targetcli only writes kernel-side state, so there is no user-space process to
# wait on. Sleep in the background so the TERM trap runs promptly on stop.
sleep infinity &
wait $!
