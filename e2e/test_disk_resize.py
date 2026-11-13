"""
E2E tests for disk resize (PUT /vms/{vm_id}/disks/{disk_id}/resize).

Resizes a disk attached to a Shutdown VM on a real Cloud Hypervisor node and
checks both the API response and the backing file's size on the VM's node.
The disk is a per-test sparse file, never a shared image.
"""

import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_objects import (
    create as create_storage_object,
)
from qarax_api_client.api.storage_objects import (
    delete as delete_storage_object,
)
from qarax_api_client.api.storage_pools import (
    attach_host as attach_pool_host,
)
from qarax_api_client.api.storage_pools import (
    create as create_pool,
)
from qarax_api_client.api.storage_pools import (
    delete as delete_pool,
)
from qarax_api_client.api.vms import (
    attach_disk,
    resize_disk,
)
from qarax_api_client.api.vms import (
    create as create_vm,
)
from qarax_api_client.api.vms import (
    delete as delete_vm,
)
from qarax_api_client.api.vms import (
    get as get_vm,
)
from qarax_api_client.api.vms import (
    start as start_vm,
)
from qarax_api_client.api.vms import (
    stop as stop_vm,
)
from qarax_api_client.models import (
    Hypervisor,
    NewStoragePool,
    NewVm,
    StoragePoolType,
    VmStatus,
)
from qarax_api_client.models.attach_disk_request import AttachDiskRequest
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest
from qarax_api_client.models.disk_resize_request import DiskResizeRequest
from qarax_api_client.models.new_storage_object import NewStorageObject
from qarax_api_client.models.storage_object_type import StorageObjectType

from helpers import (
    AUTH_HEADERS,
    QARAX_URL,
    call_api,
    call_api_detailed,
    compose_container,
    create_sparse_file_on_nodes,
    docker,
    remove_file_from_nodes,
    wait_for_status,
)
from helpers import up_hosts as _up_hosts

MIB = 1024 * 1024


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


async def _make_vm(c, test_id):
    vm_id_raw = await call_api(
        create_vm,
        client=c,
        body=NewVm(
            name=f"e2e-drs-vm-{test_id}",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * MIB,
        ),
    )
    return UUID(str(vm_id_raw).strip('"'))


async def _make_local_pool(c, test_id, hosts):
    """Create a local storage pool attached to all UP hosts, return pool_id."""
    pool_id_raw = await call_api(
        create_pool,
        client=c,
        body=NewStoragePool(
            name=f"e2e-drs-pool-{test_id}",
            pool_type=StoragePoolType.LOCAL,
            config={"path": f"/var/lib/qarax/e2e-drs-{test_id}"},
        ),
    )
    pool_id = UUID(str(pool_id_raw).strip('"'))
    for host in _up_hosts(hosts):
        await call_api(
            attach_pool_host,
            client=c,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=host.id),
        )
    return pool_id


async def _make_disk(c, test_id, pool_id, path, size_bytes):
    disk_id_raw = await call_api(
        create_storage_object,
        client=c,
        body=NewStorageObject(
            name=f"e2e-drs-disk-{test_id}",
            storage_pool_id=str(pool_id),
            object_type=StorageObjectType.DISK,
            size_bytes=size_bytes,
            config={"path": path},
        ),
    )
    return UUID(str(disk_id_raw).strip('"'))


@pytest.mark.asyncio
async def test_resize_disk_shutdown_vm(client):
    """
    Resize a disk attached to a Shutdown VM.

    Creates a local pool, attaches a disk pointing to an isolated sparse file,
    starts and stops the VM to reach Shutdown state, then resizes the disk
    from its recorded size to double that size. Verifies the returned
    StorageObject and the backing file on the VM's node both have the new size.
    """
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = _up_hosts(await call_api(list_hosts, client=c))
        assert hosts, "No UP hosts available"
        pool_id = disk_id = vm_id = None
        disk_path = f"/var/lib/qarax/e2e-drs-{test_id}/resize-disk.raw"
        try:
            pool_id = await _make_local_pool(c, test_id, hosts)

            initial_size = 256 * MIB
            new_size = initial_size * 2
            create_sparse_file_on_nodes(disk_path, initial_size)

            disk_id = await _make_disk(
                c,
                test_id,
                pool_id,
                path=disk_path,
                size_bytes=initial_size,
            )
            vm_id = await _make_vm(c, test_id)
            attached = await call_api(
                attach_disk,
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            logical_name = attached.logical_name

            # Boot and stop to get the VM into Shutdown state (assigns a host).
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)
            await call_api(stop_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)

            resp = await call_api_detailed(
                resize_disk,
                client=c,
                vm_id=vm_id,
                disk_id=logical_name,
                body=DiskResizeRequest(new_size_bytes=new_size),
            )
            assert resp.status_code == 200, (
                f"Disk resize failed: {resp.status_code} {resp.content.decode()}"
            )
            updated = resp.parsed
            assert updated.size_bytes == new_size, (
                f"Expected size_bytes={new_size}, got {updated.size_bytes}"
            )

            # VM should still be Shutdown after resize.
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.SHUTDOWN, (
                f"VM should remain Shutdown after disk resize, got {vm.status}"
            )

            # The node grew the backing file, not just the database record.
            vm_host = next(h for h in hosts if h.id == vm.host_id)
            stat = docker(
                "exec", compose_container(vm_host.address), "stat", "-c", "%s", disk_path
            )
            assert int(stat.stdout) == new_size, (
                f"Expected {disk_path} on {vm_host.address} to be {new_size} bytes, "
                f"got {stat.stdout.strip()}"
            )
        finally:
            if vm_id:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if disk_id:
                await call_api(delete_storage_object, client=c, object_id=disk_id)
            if pool_id:
                await call_api(delete_pool, client=c, pool_id=pool_id)
            remove_file_from_nodes(disk_path)
