#!/usr/bin/env bash
#
# Demo: BLOCK (iSCSI) storage pool backed by LIO targetcli
#
# Spins up a LIO iSCSI target container in the qarax compose network, registers
# it in qarax as a BLOCK storage pool, registers its LUN as a disk object, and
# creates a VM that boots from the iSCSI LUN.
#
# Prerequisites:
#   - qarax stack running (make run-local)
#   - qarax CLI on PATH
#   - docker / docker compose

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
TARGET_IQN="${TARGET_IQN:-iqn.2024-01.qarax:demo}"
PORTAL_HOST="${PORTAL_HOST:-iscsi-target}"
PORTAL_PORT="${PORTAL_PORT:-3260}"
LUN_SIZE_BYTES="${LUN_SIZE_BYTES:-1073741824}"
POOL_NAME="block-demo-$$"
DISK_NAME="block-demo-lun0-$$"

ensure_stack "${SERVER}"

QARAX_BIN="$(find_qarax_bin)"
[[ -n "${QARAX_BIN}" ]] || die "qarax CLI not found on PATH or in target/"

echo -e "${CYAN}==> Starting LIO iSCSI target sidecar${NC}"
(cd "${REPO_ROOT}/e2e" && docker compose \
    -f docker-compose.yml \
    -f ../demos/block-storage/compose.yml \
    up -d --build iscsi-target)

# Wait for the target to come up
elapsed=0
until docker exec e2e-iscsi-target-1 ss -ltn 2>/dev/null | grep -q :3260; do
    sleep 2
    elapsed=$((elapsed + 2))
    [[ $elapsed -gt 60 ]] && die "iSCSI target did not start in time"
done
echo -e "${GREEN}   iSCSI target listening on ${PORTAL_HOST}:${PORTAL_PORT}${NC}"

echo -e "${CYAN}==> Creating BLOCK storage pool${NC}"
POOL_ID=$("${QARAX_BIN}" --server "${SERVER}" --output json storage pool create \
    --name "${POOL_NAME}" \
    --pool-type block \
    --portal "${PORTAL_HOST}:${PORTAL_PORT}" \
    --iqn "${TARGET_IQN}" \
    --capacity "${LUN_SIZE_BYTES}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["pool_id"])')
echo -e "${GREEN}   pool id: ${POOL_ID}${NC}"

# Shared pools auto-attach to UP hosts in a background task. Give it a moment.
sleep 3

echo -e "${CYAN}==> Registering LUN 0 as disk object${NC}"
DISK_JSON=$("${QARAX_BIN}" --server "${SERVER}" --output json storage pool register-lun \
    --pool "${POOL_ID}" \
    --name "${DISK_NAME}" \
    --lun 0 \
    --size "${LUN_SIZE_BYTES}")
DISK_ID=$(echo "${DISK_JSON}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["storage_object_id"])')
echo -e "${GREEN}   disk id: ${DISK_ID}${NC}"

echo
echo -e "${BOLD}Done.${NC} Pool ${POOL_NAME} (${POOL_ID}) is ready with one 1 GiB iSCSI LUN."
echo "You can attach this disk to a VM with:"
echo "    ${QARAX_BIN} --server ${SERVER} vm create ... --disk ${DISK_ID}"
echo
echo "Tear down the sidecar with:"
echo "    (cd ${REPO_ROOT}/e2e && docker compose -f docker-compose.yml -f ../demos/block-storage/compose.yml down iscsi-target)"
