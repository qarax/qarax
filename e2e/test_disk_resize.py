"""
E2E tests for disk resize (PUT /vms/{vm_id}/disks/{disk_id}/resize).

Covers:
  - Negative: resize disk on a running VM → 422
  - Negative: new_size_bytes <= current size → 422
  - Negative: new_size_bytes not a multiple of 1 MiB → 422
  - Negative: disk_id not found → 404
  - Positive: resize disk on a Shutdown VM (local pool, real Cloud Hypervisor node)

The positive test requires a qarax-node with KVM passthrough and a pre-existing
file at /var/lib/qarax/images/test-initramfs.gz (the same file used by hotplug
tests as a stand-in raw block device).
"""

import asyncio
import os
import subprocess
import time
import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_objects import (
    create as create_storage_object,
    delete as delete_storage_object,
)
from qarax_api_client.api.storage_pools import (
    attach_host as attach_pool_host,
    create as create_pool,
    delete as delete_pool,
)
from qarax_api_client.api.vms import (
    attach_disk,
    create as create_vm,
    delete as delete_vm,
    get as get_vm,
    resize_disk,
    start as start_vm,
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

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
VM_OPERATION_TIMEOUT = 30

MIB = 1024 * 1024
GIB = 1024 * MIB

# Initial size used for disk storage objects in negative tests.
DISK_SIZE = 2 * GIB

# Pre-existing file on the node used as a stand-in raw block device.
TEST_FILE_PATH = "/var/lib/qarax/images/test-initramfs.gz"


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


def _node_container_names():
    result = subprocess.run(
        ["docker", "ps", "--format", "{{.Names}}"],
        check=True,
        capture_output=True,
        text=True,
    )
    return [
        name for name in result.stdout.splitlines() if name.startswith("e2e-qarax-node")
    ]


def _create_sparse_disk_on_nodes(path, size_bytes):
    command = f"mkdir -p {os.path.dirname(path)} && truncate -s {size_bytes} {path}"
    for container in _node_container_names():
        subprocess.run(
            ["docker", "exec", container, "sh", "-lc", command],
            check=True,
            capture_output=True,
            text=True,
        )


def _remove_disk_from_nodes(path):
    command = f"rm -f {path}"
    for container in _node_container_names():
        subprocess.run(
            ["docker", "exec", container, "sh", "-lc", command],
            check=False,
            capture_output=True,
            text=True,
        )


async def wait_for_status(c, vm_id, expected_status, timeout=VM_OPERATION_TIMEOUT):
    start = time.time()
    while time.time() - start < timeout:
        vm = await get_vm.asyncio(client=c, vm_id=vm_id)
        if vm.status == expected_status:
            return vm
        await asyncio.sleep(0.5)
    vm = await get_vm.asyncio(client=c, vm_id=vm_id)
    raise TimeoutError(
        f"VM {vm_id} did not reach {expected_status} within {timeout}s. Current: {vm.status}"
    )


async def _make_vm(c, test_id):
    vm_id_raw = await create_vm.asyncio(
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
    """Create a local storage pool attached to all hosts, return pool_id."""
    pool_id_raw = await create_pool.asyncio(
        client=c,
        body=NewStoragePool(
            name=f"e2e-drs-pool-{test_id}",
            pool_type=StoragePoolType.LOCAL,
            config={"path": f"/var/lib/qarax/e2e-drs-{test_id}"},
        ),
    )
    pool_id = UUID(str(pool_id_raw).strip('"'))
    for host in hosts:
        await attach_pool_host.asyncio_detailed(
            client=c,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=host.id),
        )
    return pool_id


async def _make_disk(c, test_id, pool_id, path=TEST_FILE_PATH, size_bytes=DISK_SIZE):
    disk_id_raw = await create_storage_object.asyncio(
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


# Negative tests


@pytest.mark.asyncio
async def test_resize_disk_not_found_returns_404(client):
    """Requesting a non-existent disk_id returns 404."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        vm_id = None
        try:
            vm_id = await _make_vm(c, test_id)
            resp = await resize_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                disk_id="nonexistent-disk",
                body=DiskResizeRequest(new_size_bytes=4 * GIB),
            )
            assert resp.status_code == 404, (
                f"Expected 404 for missing disk, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_resize_disk_size_too_small_returns_422(client):
    """Requesting a new size <= current size returns 422."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        pool_id = disk_id = vm_id = None
        try:
            pool_id = await _make_local_pool(c, test_id, hosts)
            disk_id = await _make_disk(c, test_id, pool_id, size_bytes=2 * GIB)
            vm_id = await _make_vm(c, test_id)
            await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )

            resp = await resize_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                disk_id="disk0",
                body=DiskResizeRequest(new_size_bytes=1 * GIB),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for size <= current, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if disk_id:
                await delete_storage_object.asyncio_detailed(
                    client=c, object_id=disk_id
                )
            if pool_id:
                await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)


@pytest.mark.asyncio
async def test_resize_disk_not_mib_aligned_returns_422(client):
    """Requesting a new size not aligned to 1 MiB returns 422."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        pool_id = disk_id = vm_id = None
        try:
            pool_id = await _make_local_pool(c, test_id, hosts)
            disk_id = await _make_disk(c, test_id, pool_id, size_bytes=2 * GIB)
            vm_id = await _make_vm(c, test_id)
            await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )

            resp = await resize_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                disk_id="disk0",
                body=DiskResizeRequest(new_size_bytes=2 * GIB + 500 * 1024),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for unaligned size, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if disk_id:
                await delete_storage_object.asyncio_detailed(
                    client=c, object_id=disk_id
                )
            if pool_id:
                await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)


@pytest.mark.asyncio
async def test_resize_disk_running_vm_returns_422(client):
    """Resizing a disk while the VM is Running returns 422."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        pool_id = disk_id = vm_id = None
        try:
            pool_id = await _make_local_pool(c, test_id, hosts)
            disk_id = await _make_disk(
                c, test_id, pool_id, path=TEST_FILE_PATH, size_bytes=1
            )
            vm_id = await _make_vm(c, test_id)
            await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            resp = await resize_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                disk_id="disk0",
                body=DiskResizeRequest(new_size_bytes=4 * GIB),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for running VM, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
                try:
                    await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)
                except TimeoutError:
                    pass
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if disk_id:
                await delete_storage_object.asyncio_detailed(
                    client=c, object_id=disk_id
                )
            if pool_id:
                await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)


# Positive test


@pytest.mark.asyncio
async def test_resize_disk_shutdown_vm(client):
    """
    Resize a disk attached to a Shutdown VM.

    Creates a local pool, attaches a disk pointing to an isolated sparse file,
    starts and stops the VM to reach Shutdown state,
    then resizes the disk from its recorded size to double that size.
    Verifies the returned StorageObject reflects the new size.
    """
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        pool_id = disk_id = vm_id = None
        disk_path = f"/var/lib/qarax/e2e-drs-{test_id}/resize-disk.raw"
        try:
            pool_id = await _make_local_pool(c, test_id, hosts)

            initial_size = 256 * MIB
            new_size = initial_size * 2
            _create_sparse_disk_on_nodes(disk_path, initial_size)

            disk_id = await _make_disk(
                c,
                test_id,
                pool_id,
                path=disk_path,
                size_bytes=initial_size,
            )
            vm_id = await _make_vm(c, test_id)
            attach_resp = await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            assert attach_resp.status_code == 201, (
                f"Failed to attach disk: {attach_resp.status_code}"
            )
            logical_name = attach_resp.parsed.logical_name

            # Boot and stop to get the VM into Shutdown state (assigns a host).
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)
            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)

            resp = await resize_disk.asyncio_detailed(
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
            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.SHUTDOWN, (
                f"VM should remain Shutdown after disk resize, got {vm.status}"
            )
        finally:
            if vm_id:
                try:
                    await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
                    await wait_for_status(c, vm_id, VmStatus.SHUTDOWN, timeout=10)
                except Exception:
                    pass
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if disk_id:
                try:
                    await delete_storage_object.asyncio_detailed(
                        client=c, object_id=disk_id
                    )
                except Exception as e:
                    print(f"Ignoring error during disk cleanup: {e}")
            if pool_id:
                try:
                    await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)
                except Exception as e:
                    print(f"Ignoring error during pool cleanup: {e}")
            _remove_disk_from_nodes(disk_path)
