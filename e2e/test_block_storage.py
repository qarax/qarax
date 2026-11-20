"""
E2E test for BLOCK (iSCSI) storage pools, on the bootc node VM.

The compose node containers can't log in to an iSCSI target: the kernel's
iSCSI initiator works only in the host network namespace. bootc-vm runs a
kernel of its own, so it covers the node side: the pool attaches to it
(iscsiadm discovery and login), and a VM there boots with one of the target's
LUNs as a disk, the /dev/disk/by-path link udev makes for the LUN.

The target is the compose service iscsi-target (Dockerfile.iscsi-target),
which bootc-vm's guest reaches through its NAT to the compose network.
"""

import asyncio
import time
import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.storage_objects import delete as delete_storage_object
from qarax_api_client.api.storage_pools import create as create_pool
from qarax_api_client.api.storage_pools import delete as delete_pool
from qarax_api_client.api.storage_pools import detach_host as detach_pool_host
from qarax_api_client.api.storage_pools import list_hosts as list_pool_hosts
from qarax_api_client.api.storage_pools import register_lun
from qarax_api_client.api.vms import attach_disk
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.models import (
    AttachDiskRequest,
    Hypervisor,
    NewStoragePool,
    NewVm,
    PlacementPolicy,
    RegisterLunRequest,
    StoragePoolType,
)
from qarax_api_client.models.placement_policy_required_host_labels import (
    PlacementPolicyRequiredHostLabels,
)

from helpers import (
    AUTH_HEADERS,
    BOOTC_NODE_BOOT_TIMEOUT_SECONDS,
    BOOTC_NODE_LABEL,
    QARAX_URL,
    call_api,
    compose_container,
    docker,
    exec_when_ready,
    wait_for_start,
)

# The compose service iscsi-target (e2e/docker-compose.yml).
TARGET_IQN = "iqn.2024-01.io.qarax:e2e"
TARGET_LUN_FILE = "/var/lib/qarax-block/lun0.img"
LUN_SIZE = 1024**3
# A new pool attaches to every UP host in the background, one at a time; the
# node containers' logins fail.
ATTACH_TIMEOUT_SECONDS = 300
# A VM on the bootc node is nested twice on CI runners.
VM_START_TIMEOUT_SECONDS = 300

pytestmark = pytest.mark.timeout(
    BOOTC_NODE_BOOT_TIMEOUT_SECONDS
    + ATTACH_TIMEOUT_SECONDS
    + 2 * VM_START_TIMEOUT_SECONDS
    + 300
)


def _target_ip():
    """The iSCSI target's address on the compose network: portals are IPs."""
    ip = docker(
        "inspect",
        "-f",
        "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
        compose_container("iscsi-target"),
    ).stdout.strip()
    assert ip, "the iscsi-target container has no address"
    return ip


def _on_bootc_node(command):
    """Run a shell command on the bootc node VM, for a failure message."""
    out = docker(
        "exec", compose_container("bootc-vm"), "node-ssh", command, check=False
    )
    return f"--- bootc node: {command} ---\n{out.stdout}{out.stderr}"


async def _wait_until_attached(c, pool_id, host_id):
    deadline = time.monotonic() + ATTACH_TIMEOUT_SECONDS
    while True:
        hosts = await call_api(list_pool_hosts, client=c, pool_id=pool_id)
        if any(h.host_id == host_id for h in hosts):
            return
        if time.monotonic() >= deadline:
            raise AssertionError(
                f"the BLOCK pool did not attach to the bootc node within "
                f"{ATTACH_TIMEOUT_SECONDS}s\n"
                + _on_bootc_node(
                    "iscsiadm -m session; "
                    "journalctl -u qarax-node -u iscsid --no-pager | tail -n 40"
                )
            )
        await asyncio.sleep(2)


@pytest.mark.asyncio
async def test_vm_on_bootc_node_boots_with_a_block_pool_lun(bootc_host_id):
    """
    A BLOCK pool attaches to the bootc node, and a VM placed there starts with
    one of the pool's LUNs as its disk: what the guest writes to that disk
    lands in the target's LUN file.

    The pool's portal is given without a port, so the LUN's device path holds
    only if qarax stores the portal as udev names the LUN (IP:3260).
    """
    test_id = uuid.uuid4().hex[:8]
    marker = f"qarax-e2e-{test_id}"
    pool_id = disk_id = vm_id = None
    async with Client(base_url=QARAX_URL, headers=AUTH_HEADERS) as c:
        try:
            raw = await call_api(
                create_pool,
                client=c,
                body=NewStoragePool(
                    name=f"e2e-block-{test_id}",
                    pool_type=StoragePoolType.BLOCK,
                    config={"portal": _target_ip(), "iqn": TARGET_IQN},
                ),
            )
            pool_id = UUID(str(raw).strip('"'))
            await _wait_until_attached(c, pool_id, bootc_host_id)

            lun = await call_api(
                register_lun,
                client=c,
                pool_id=pool_id,
                body=RegisterLunRequest(
                    name=f"e2e-block-lun-{test_id}", lun=0, size_bytes=LUN_SIZE
                ),
            )
            disk_id = lun.storage_object_id

            raw = await call_api(
                create_vm,
                client=c,
                body=NewVm(
                    name=f"e2e-block-vm-{test_id}",
                    hypervisor=Hypervisor.CLOUD_HV,
                    boot_vcpus=1,
                    max_vcpus=1,
                    memory_size=256 * 1024 * 1024,
                    guest_agent=True,
                    placement_policy=PlacementPolicy(
                        required_host_labels=PlacementPolicyRequiredHostLabels.from_dict(
                            BOOTC_NODE_LABEL
                        )
                    ),
                ),
            )
            vm_id = UUID(str(raw).strip('"'))
            await call_api(
                attach_disk,
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )

            started = await call_api(start_vm, client=c, vm_id=vm_id)
            try:
                await wait_for_start(
                    c, vm_id, started.job_id, timeout=VM_START_TIMEOUT_SECONDS
                )
            except TimeoutError as e:
                raise AssertionError(
                    f"{e}\n"
                    + _on_bootc_node(
                        f"tail -c 4000 /var/lib/qarax/vms/{vm_id}.console.log; "
                        f"journalctl -u qarax-node --no-pager | grep {vm_id} | tail -n 20"
                    )
                ) from e

            # The LUN is the guest's only disk: write the marker to it, then
            # read it from the target's LUN file.
            result = await exec_when_ready(
                c,
                vm_id,
                "for d in /sys/block/vd*; do "
                f'  [ "$(cat $d/size)" = {LUN_SIZE // 512} ] && '
                f"    printf %s {marker} | busybox dd of=/dev/${{d##*/}} conv=fsync && exit 0; "
                "done; exit 1",
                timeout=VM_START_TIMEOUT_SECONDS,
            )
            assert result.exit_code == 0, (
                f"the guest did not write to a {LUN_SIZE}-byte disk: "
                f"{result.stdout!r} {result.stderr!r}"
            )
            written = docker(
                "exec",
                compose_container("iscsi-target"),
                "head",
                "-c",
                str(len(marker)),
                TARGET_LUN_FILE,
            ).stdout
            assert written == marker, (
                f"the LUN file starts with {written!r}, not the guest's {marker!r}"
            )
        finally:
            if vm_id:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if disk_id:
                await call_api(delete_storage_object, client=c, object_id=disk_id)
            if pool_id:
                # Detaching logs the node out of the target.
                for host in await call_api(list_pool_hosts, client=c, pool_id=pool_id):
                    await call_api(
                        detach_pool_host,
                        client=c,
                        pool_id=pool_id,
                        host_id=host.host_id,
                    )
                await call_api(delete_pool, client=c, pool_id=pool_id)
