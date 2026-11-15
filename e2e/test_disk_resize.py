"""
E2E tests for disk images on a real Cloud Hypervisor node.

Resize (PUT /vms/{vm_id}/disks/{disk_id}/resize): a raw or qcow2 image
attached to a Shutdown VM, checking the API response and the image's virtual
size on the VM's node, and a raw image of a running VM, checking the guest
sees the new size. Format: a guest can't make its raw disk be opened as qcow2
by writing a qcow2 header to it. Each disk is a per-test image, never a shared
one.
"""

import json
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
    remove_disk,
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
    create_image_on_nodes,
    docker,
    exec_when_ready,
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
            guest_agent=True,
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


async def _resize(c, vm_id, logical_name, new_size):
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
    assert resp.parsed.size_bytes == new_size, (
        f"Expected size_bytes={new_size}, got {resp.parsed.size_bytes}"
    )


@pytest.mark.asyncio
@pytest.mark.parametrize("image_format", ["raw", "qcow2"])
@pytest.mark.parametrize("started", [True, False], ids=["shutdown", "never-started"])
async def test_resize_disk_stopped_vm(client, image_format, started):
    """
    Resize a disk attached to a stopped VM: one started and stopped, or one
    never started, which its node doesn't know yet.

    Creates a local pool and a disk backed by a per-test image, then doubles
    the disk. Verifies the returned StorageObject and the image's virtual size
    on the VM's node: for qcow2 that lives in the image header, which growing
    the file alone leaves as it was.
    """
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = _up_hosts(await call_api(list_hosts, client=c))
        assert hosts, "No UP hosts available"
        pool_id = disk_id = vm_id = None
        disk_path = f"/var/lib/qarax/e2e-drs-{test_id}/resize-disk.{image_format}"
        try:
            pool_id = await _make_local_pool(c, test_id, hosts)

            initial_size = 256 * MIB
            new_size = initial_size * 2
            create_image_on_nodes(disk_path, initial_size, image_format)

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

            stopped_status = VmStatus.CREATED
            if started:
                await call_api(start_vm, client=c, vm_id=vm_id)
                await wait_for_status(c, vm_id, VmStatus.RUNNING)
                await call_api(stop_vm, client=c, vm_id=vm_id)
                await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)
                stopped_status = VmStatus.SHUTDOWN

            await _resize(c, vm_id, attached.logical_name, new_size)

            # The VM stays stopped.
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == stopped_status, (
                f"VM should remain {stopped_status} after disk resize, got {vm.status}"
            )

            # The node grew the image, not just the database record.
            vm_host = next(h for h in hosts if h.id == vm.host_id)
            info = docker(
                "exec",
                compose_container(vm_host.address),
                "qemu-img",
                "info",
                "-f",
                image_format,
                "--output=json",
                disk_path,
            )
            virtual_size = json.loads(info.stdout)["virtual-size"]
            assert virtual_size == new_size, (
                f"Expected {disk_path} on {vm_host.address} to be {new_size} bytes, "
                f"got {virtual_size}"
            )
        finally:
            if vm_id:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if disk_id:
                await call_api(delete_storage_object, client=c, object_id=disk_id)
            if pool_id:
                await call_api(delete_pool, client=c, pool_id=pool_id)
            remove_file_from_nodes(disk_path)


@pytest.mark.asyncio
async def test_resize_disk_running_vm(client):
    """
    Resize a disk of a running VM: the guest sees the new size without a
    restart, and the VM keeps running.
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
            create_image_on_nodes(disk_path, initial_size, "raw")
            disk_id = await _make_disk(
                c, test_id, pool_id, path=disk_path, size_bytes=initial_size
            )
            vm_id = await _make_vm(c, test_id)
            attached = await call_api(
                attach_disk,
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            # The guest's only disk; its size is in 512-byte sectors.
            size_in_guest = "cat /sys/block/vda/size"
            result = await exec_when_ready(c, vm_id, size_in_guest)
            assert result.stdout.strip() == str(initial_size // 512), result

            await _resize(c, vm_id, attached.logical_name, new_size)

            result = await exec_when_ready(
                c,
                vm_id,
                "for i in 1 2 3 4 5 6 7 8 9 10; do "
                f'  [ "$({size_in_guest})" = {new_size // 512} ] && exit 0; sleep 1; '
                f"done; {size_in_guest}; exit 1",
            )
            assert result.exit_code == 0, (
                f"the guest still sees {result.stdout.strip()} sectors, "
                f"not {new_size // 512}"
            )
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, vm.status
        finally:
            if vm_id:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if disk_id:
                await call_api(delete_storage_object, client=c, object_id=disk_id)
            if pool_id:
                await call_api(delete_pool, client=c, pool_id=pool_id)
            remove_file_from_nodes(disk_path)


@pytest.mark.asyncio
async def test_guest_cannot_turn_its_raw_disk_into_qcow2(client):
    """
    A guest writes the qcow2 magic over the start of its raw disk; plugged in
    again, the disk is still raw, with the guest's bytes at its start. Its
    format was recorded when the VM first started, before the guest could
    write to it. The node used to read it from the image every time it built
    the disk's config, so it then opened the image as qcow2 with a header the
    guest wrote, one that could claim any size.
    """
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = _up_hosts(await call_api(list_hosts, client=c))
        assert hosts, "No UP hosts available"
        pool_id = disk_id = vm_id = None
        disk_path = f"/var/lib/qarax/e2e-drs-{test_id}/planted.raw"
        size = 64 * MIB
        try:
            pool_id = await _make_local_pool(c, test_id, hosts)
            create_image_on_nodes(disk_path, size, "raw")
            disk_id = await _make_disk(c, test_id, pool_id, path=disk_path, size_bytes=size)
            vm_id = await _make_vm(c, test_id)
            attached = await call_api(
                attach_disk,
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            result = await exec_when_ready(
                c,
                vm_id,
                "printf 'QFI\\373' | busybox dd of=/dev/vda conv=notrunc 2>/dev/null && busybox sync",
            )
            assert result.exit_code == 0, result

            rm_resp = await remove_disk.asyncio_detailed(
                client=c, vm_id=vm_id, device_id=attached.logical_name
            )
            assert rm_resp.status_code == 204, rm_resp.content.decode()
            # Right away: removal returns once the guest has ejected the disk,
            # so its id is free again.
            attach_resp = await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            assert attach_resp.status_code == 201, (
                f"Re-attaching the disk failed: {attach_resp.status_code} "
                f"{attach_resp.content.decode()}"
            )

            # The guest sees the disk again, its size and first bytes unchanged.
            result = await exec_when_ready(
                c,
                vm_id,
                "for i in 1 2 3 4 5 6 7 8 9 10; do "
                f'  [ "$(cat /sys/block/vda/size 2>/dev/null)" = {size // 512} ] && '
                "    exec busybox od -An -tx1 -N4 /dev/vda; "
                "  sleep 1; "
                "done; exit 1",
            )
            assert result.exit_code == 0, f"the guest never saw the disk again: {result}"
            assert result.stdout.split() == ["51", "46", "49", "fb"], result.stdout
        finally:
            if vm_id:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if disk_id:
                await call_api(delete_storage_object, client=c, object_id=disk_id)
            if pool_id:
                await call_api(delete_pool, client=c, pool_id=pool_id)
            remove_file_from_nodes(disk_path)
