"""
E2E tests for VMs whose node restarted.

A node that restarts the way a host reboot leaves it has lost its VM
processes, and forgets the VMs it doesn't find running. Starting one of them
again, stopped or killed while running, defines it on the node from its
record first.

The test restarts qarax-node containers, so it must run against the standard
e2e compose project. A container restart, unlike a host reboot, also leaves
an image VM's OverlayBD device in the host kernel's configfs, which its new
definition has to replace.
"""

import uuid

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_pools import create as create_pool
from qarax_api_client.api.storage_pools import delete as delete_pool
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.api.vms import stop as stop_vm
from qarax_api_client.models import (
    Hypervisor,
    NewStoragePool,
    NewVm,
    StoragePoolType,
    VmStatus,
)

from helpers import (
    AUTH_HEADERS,
    QARAX_URL,
    REGISTRY_INTERNAL_URL,
    attach_overlaybd_pool,
    call_api,
    compose,
    reinit_host,
    up_hosts,
    wait_for_start,
    wait_for_status,
)
from test_storage_types import wait_for_job

VM_OPERATION_TIMEOUT = 60
# The VM monitor finds a VM gone from its node on its next 30 s tick.
VM_MONITOR_TIMEOUT = 90


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


def _new_vm(name, hypervisor, **kwargs):
    return NewVm(
        name=name,
        hypervisor=hypervisor,
        boot_vcpus=1,
        max_vcpus=1,
        memory_size=256 * 1024 * 1024,
        **kwargs,
    )


async def _start(c, vm_id):
    job = await call_api(start_vm, client=c, vm_id=vm_id)
    await wait_for_start(c, vm_id, job.job_id, timeout=VM_OPERATION_TIMEOUT)


@pytest.mark.asyncio
async def test_vms_start_again_after_their_node_restarts(
    client, bootable_oci_image_ref
):
    """A stopped Cloud Hypervisor VM, a running Firecracker VM and a running
    VM booted from an OCI image, all lost when their nodes restart, start
    again."""
    suffix = uuid.uuid4().hex[:6]
    vm_ids = []
    pool_id = None

    async with client as c:
        try:
            stopped_id = await call_api(
                create_vm,
                client=c,
                body=_new_vm(f"e2e-restart-stopped-{suffix}", Hypervisor.CLOUD_HV),
            )
            vm_ids.append(stopped_id)
            await _start(c, stopped_id)
            await call_api(stop_vm, client=c, vm_id=stopped_id)
            await wait_for_status(
                c, stopped_id, VmStatus.SHUTDOWN, timeout=VM_OPERATION_TIMEOUT
            )

            firecracker_id = await call_api(
                create_vm,
                client=c,
                body=_new_vm(f"e2e-restart-fc-{suffix}", Hypervisor.FIRECRACKER),
            )
            vm_ids.append(firecracker_id)
            await _start(c, firecracker_id)

            hosts = (await call_api(list_hosts, client=c)).items
            pool_id = uuid.UUID(
                str(
                    await call_api(
                        create_pool,
                        client=c,
                        body=NewStoragePool(
                            name=f"e2e-restart-obd-{suffix}",
                            pool_type=StoragePoolType.OVERLAY_BD,
                            config={"url": f"http://{REGISTRY_INTERNAL_URL}"},
                        ),
                    )
                )
            )
            await attach_overlaybd_pool(c, up_hosts(hosts), pool_id)
            image = await call_api(
                create_vm,
                client=c,
                body=_new_vm(
                    f"e2e-restart-image-{suffix}",
                    Hypervisor.CLOUD_HV,
                    image_ref=bootable_oci_image_ref,
                ),
            )
            vm_ids.append(image.vm_id)
            await wait_for_job(c, image.job_id)
            await _start(c, image.vm_id)

            # The e2e hosts are registered with their compose service as address.
            addresses = {h.id: h.address for h in hosts}
            vm_hosts = {
                (await call_api(get_vm, client=c, vm_id=vm_id)).host_id
                for vm_id in vm_ids
            }
            for host_id in vm_hosts:
                compose("restart", addresses[host_id])
            for host_id in vm_hosts:
                await reinit_host(c, host_id)

            # The VM monitor finds the running VMs gone from their node. It
            # doesn't ask about stopped ones, so that one stays shutdown.
            for vm_id in (firecracker_id, image.vm_id):
                await wait_for_status(
                    c, vm_id, VmStatus.UNKNOWN, timeout=VM_MONITOR_TIMEOUT
                )
            for vm_id in vm_ids:
                await _start(c, vm_id)
        finally:
            for vm_id in vm_ids:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if pool_id is not None:
                await call_api(delete_pool, client=c, pool_id=pool_id)
