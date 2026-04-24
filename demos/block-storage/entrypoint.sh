#!/usr/bin/env bash
# LIO iSCSI target bootstrap for the BLOCK storage demo.
#
# Exports one fileio-backed LUN (default: 1 GiB) over iSCSI with a wide-open
# ACL (demo_mode enabled). IQN and LUN size are configurable via env vars:
#
#   TARGET_IQN   iSCSI target IQN (default: iqn.2024-01.qarax:demo)
#   LUN_SIZE     backing file size in bytes (default: 1073741824 = 1 GiB)
#   LUN_PATH     backing file path (default: /var/lib/qarax-block/lun0.img)

set -euo pipefail

TARGET_IQN="${TARGET_IQN:-iqn.2024-01.qarax:demo}"
LUN_PATH="${LUN_PATH:-/var/lib/qarax-block/lun0.img}"
LUN_SIZE="${LUN_SIZE:-1073741824}"

echo "==> BLOCK demo target: ${TARGET_IQN}  path=${LUN_PATH}  size=${LUN_SIZE}"

mkdir -p "$(dirname "${LUN_PATH}")"

if [[ ! -f "${LUN_PATH}" ]]; then
    echo "==> Allocating backing file"
    truncate -s "${LUN_SIZE}" "${LUN_PATH}"
fi

echo "==> Configuring LIO via targetcli"
targetcli <<EOF
/backstores/fileio create demo_lun0 ${LUN_PATH} ${LUN_SIZE}
/iscsi create ${TARGET_IQN}
/iscsi/${TARGET_IQN}/tpg1/luns create /backstores/fileio/demo_lun0 0
/iscsi/${TARGET_IQN}/tpg1 set attribute authentication=0 demo_mode_write_protect=0 generate_node_acls=1 cache_dynamic_acls=1
/iscsi/${TARGET_IQN}/tpg1/portals create 0.0.0.0 3260
saveconfig
exit
EOF

echo "==> Target ready on port 3260"

# Keep the container alive; targetcli writes kernel-side state so there is no
# long-running user-space process to exec into. A simple wait loop is enough.
tail -f /dev/null
