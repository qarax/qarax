"""
E2E tests for VM snapshot support.

These tests verify snapshot lifecycle against a real Cloud Hypervisor VM:
- Creating a snapshot of a running VM (pause → snapshot → resume)
- Listing snapshots for a VM
- Snapshot record persists with status 'ready' after success
- Restoring a stopped VM from a snapshot
- Deleting a snapshot removes its backup record and its data on the node
- A restore brings back the VM's disk as it was at the snapshot (Cloud
  Hypervisor and Firecracker), and a VM with a disk a snapshot can't capture
  isn't snapshotted
"""

import uuid
from contextlib import asynccontextmanager
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_pools import attach_host as attach_pool_host
from qarax_api_client.api.storage_pools import create as create_pool
from qarax_api_client.api.storage_pools import delete as delete_pool
from qarax_api_client.api.storage_objects import create as create_storage_object
from qarax_api_client.api.storage_objects import delete as delete_storage_object
from qarax_api_client.api.vms import (
    create as create_vm,
)
from qarax_api_client.api.vms import (
    attach_disk,
    create_snapshot,
    delete_snapshot,
    list_snapshots,
    restore,
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
    HostStatus,
    Hypervisor,
    NewStoragePool,
    NewVm,
    StoragePoolType,
    VmStatus,
)
from qarax_api_client.models.attach_disk_request import AttachDiskRequest
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest
from qarax_api_client.models.new_storage_object import NewStorageObject
from qarax_api_client.models.storage_object_type import StorageObjectType
from qarax_api_client.models.create_snapshot_request import CreateSnapshotRequest
from qarax_api_client.models.restore_request import RestoreRequest
from qarax_api_client.models.snapshot_status import SnapshotStatus

VM_OPERATION_TIMEOUT = 60

from helpers import (
    AUTH_HEADERS,
    NODE_CONTAINER_PREFIX,
    QARAX_URL,
    call_api,
    call_api_detailed,
    create_image_on_nodes,
    exec_when_ready,
    path_exists_in_any,
    remove_file_from_nodes,
    wait_for_status,
)

MIB = 1024 * 1024


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


@pytest.fixture(scope="module")
def snapshot_storage_pool():
    """Create a local storage pool for snapshot tests and attach it to the host."""
    with Client(base_url=QARAX_URL, headers=AUTH_HEADERS) as c:
        hosts = [h for h in list_hosts.sync(client=c).items if h.status == HostStatus.UP]
        assert hosts and len(hosts) > 0, "No UP hosts registered"
        host_id = hosts[0].id

        import uuid
        pool_id_raw = create_pool.sync(
            client=c,
            body=NewStoragePool(
                name=f"e2e-snapshot-pool-{uuid.uuid4().hex[:8]}",
                pool_type=StoragePoolType.LOCAL,
                config={"path": "/var/lib/qarax/snapshots"},
            ),
        )
        assert pool_id_raw is not None, "Failed to create snapshot storage pool"
        pool_id = uuid.UUID(str(pool_id_raw).strip('"'))

        attach_pool_host.sync_detailed(
            client=c,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=host_id),
        )

        yield pool_id

        delete_pool.sync_detailed(client=c, pool_id=pool_id)


@pytest.mark.asyncio
async def test_snapshot_full_lifecycle(client, snapshot_storage_pool):
    """
    Full snapshot lifecycle with a real running VM:
    create VM → start → create snapshot → verify ready → list → stop → delete.
    """
    async with client as c:
        new_vm = NewVm(
            name="e2e-snap-lifecycle",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        result = await create_vm.asyncio(client=c, body=new_vm)
        vm_id = uuid.UUID(str(result).strip('"'))

        try:
            # Start the VM and wait for RUNNING
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT)

            # Create a snapshot
            snapshot = await create_snapshot.asyncio(client=c, vm_id=vm_id, body=CreateSnapshotRequest())
            assert snapshot is not None, "Expected snapshot object, got None"
            assert snapshot.vm_id == vm_id
            assert snapshot.status == SnapshotStatus.READY
            assert snapshot.storage_object_id is not None

            # List snapshots — should show the one we just created
            snapshots = await list_snapshots.asyncio(client=c, vm_id=vm_id)
            assert snapshots is not None
            assert len(snapshots) == 1, f"Expected 1 snapshot, got {len(snapshots)}"
            assert snapshots[0].id == snapshot.id
            assert snapshots[0].status == SnapshotStatus.READY

            # VM should still be running after snapshot (pause → snapshot → resume)
            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, (
                f"VM should be RUNNING after snapshot, got: {vm.status}"
            )

            # Stop the VM
            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)

        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_snapshot_restore(client, snapshot_storage_pool):
    """
    Restore a VM from a snapshot:
    create VM → start → snapshot → stop → restore → verify RUNNING → stop → delete.
    """
    async with client as c:
        new_vm = NewVm(
            name="e2e-snap-restore",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        result = await create_vm.asyncio(client=c, body=new_vm)
        vm_id = uuid.UUID(str(result).strip('"'))

        try:
            # Start the VM and wait for RUNNING
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT)

            # Create a snapshot
            snapshot = await create_snapshot.asyncio(client=c, vm_id=vm_id, body=CreateSnapshotRequest())
            assert snapshot is not None, "Expected snapshot object, got None"
            assert snapshot.status == SnapshotStatus.READY

            # Stop the VM
            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.SHUTDOWN, timeout=VM_OPERATION_TIMEOUT)

            # Restore from snapshot
            restored_vm = await restore.asyncio(
                client=c, vm_id=vm_id, body=RestoreRequest(snapshot_id=snapshot.id)
            )
            assert restored_vm is not None, "Expected VM object from restore, got None"
            assert restored_vm.status == VmStatus.RUNNING, (
                f"Expected RUNNING after restore, got: {restored_vm.status}"
            )

            # Stop before cleanup
            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)

        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_snapshot_delete_removes_its_data(client, snapshot_storage_pool):
    async with client as c:
        httpx_client = c.get_async_httpx_client()
        new_vm = NewVm(
            name=f"e2e-snap-delete-{uuid.uuid4().hex[:8]}",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        vm_id = uuid.UUID(str(await create_vm.asyncio(client=c, body=new_vm)).strip('"'))

        try:
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT)

            snapshot = await create_snapshot.asyncio(
                client=c,
                vm_id=vm_id,
                body=CreateSnapshotRequest(storage_pool_id=snapshot_storage_pool),
            )
            assert snapshot is not None and snapshot.status == SnapshotStatus.READY
            resp = await httpx_client.get(
                f"{QARAX_URL}/v1/storage-objects/{snapshot.storage_object_id}"
            )
            assert resp.status_code == 200, resp.text
            snapshot_dir = resp.json()["config"]["path"]
            assert path_exists_in_any(NODE_CONTAINER_PREFIX, snapshot_dir)

            resp = await delete_snapshot.asyncio_detailed(
                client=c, vm_id=vm_id, snapshot_id=snapshot.id
            )
            assert resp.status_code.value == 204, resp.content

            assert await list_snapshots.asyncio(client=c, vm_id=vm_id) == []
            resp = await httpx_client.get(f"{QARAX_URL}/v1/backups", params={"backup_type": "vm"})
            assert resp.status_code == 200, resp.text
            assert all(b["snapshot_id"] != str(snapshot.id) for b in resp.json()["items"])
            resp = await httpx_client.get(
                f"{QARAX_URL}/v1/storage-objects/{snapshot.storage_object_id}"
            )
            assert resp.status_code == 404, resp.text
            assert not path_exists_in_any(NODE_CONTAINER_PREFIX, snapshot_dir)
        finally:
            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@asynccontextmanager
async def _running_vm_with_disk(c, hypervisor, image_format):
    """A running VM with the guest agent and a 64 MiB image of its own
    (`raw` or `qcow2`) as its only disk, /dev/vda, in a local pool of its own
    on the VM's host. Deleting the VM deletes its snapshots."""
    test_id = uuid.uuid4().hex[:8]
    pool_path = f"/var/lib/qarax/e2e-snapdisk-{test_id}"
    disk_path = f"{pool_path}/disk.img"
    vm_id = pool_id = disk_id = None
    try:
        vm_id = UUID(
            str(
                await call_api(
                    create_vm,
                    client=c,
                    body=NewVm(
                        name=f"e2e-snapdisk-{test_id}",
                        hypervisor=hypervisor,
                        boot_vcpus=1,
                        max_vcpus=1,
                        memory_size=256 * MIB,
                        guest_agent=True,
                    ),
                )
            ).strip('"')
        )
        vm = await call_api(get_vm, client=c, vm_id=vm_id)
        pool_id = UUID(
            str(
                await call_api(
                    create_pool,
                    client=c,
                    body=NewStoragePool(
                        name=f"e2e-snapdisk-{test_id}",
                        pool_type=StoragePoolType.LOCAL,
                        config={"path": pool_path},
                    ),
                )
            ).strip('"')
        )
        await call_api(
            attach_pool_host,
            client=c,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=vm.host_id),
        )
        create_image_on_nodes(disk_path, 64 * MIB, image_format)
        disk_id = UUID(
            str(
                await call_api(
                    create_storage_object,
                    client=c,
                    body=NewStorageObject(
                        name=f"e2e-snapdisk-{test_id}",
                        storage_pool_id=str(pool_id),
                        object_type=StorageObjectType.DISK,
                        size_bytes=64 * MIB,
                        config={"path": disk_path},
                    ),
                )
            ).strip('"')
        )
        # Before the start: Firecracker drives can't be hotplugged.
        await call_api(
            attach_disk,
            client=c,
            vm_id=vm_id,
            body=AttachDiskRequest(storage_object_id=disk_id),
        )
        await call_api(start_vm, client=c, vm_id=vm_id)
        await wait_for_status(c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT)
        yield vm_id
    finally:
        if vm_id:
            await call_api(delete_vm, client=c, vm_id=vm_id)
        if disk_id:
            await call_api(delete_storage_object, client=c, object_id=disk_id)
        if pool_id:
            await call_api(delete_pool, client=c, pool_id=pool_id)
        remove_file_from_nodes(disk_path)


def _write_marker(marker):
    return f"printf {marker} | busybox dd of=/dev/vda conv=notrunc 2>/dev/null && busybox sync"


@pytest.mark.asyncio
@pytest.mark.parametrize("hypervisor", [Hypervisor.CLOUD_HV, Hypervisor.FIRECRACKER])
async def test_restore_brings_back_the_disk(client, hypervisor):
    """
    A restore puts the VM's disk back as it was at the snapshot, with its
    memory: over a disk the guest wrote to since, the guest's page cache and
    filesystems wouldn't match what's on it. The guest writes a marker to its
    disk, the VM is snapshotted, the guest overwrites the marker and the VM
    stops; once restored, the guest reads the first marker off the disk, past
    its page cache.
    """
    async with client as c:
        async with _running_vm_with_disk(c, hypervisor, "raw") as vm_id:
            result = await exec_when_ready(c, vm_id, _write_marker("before"))
            assert result.exit_code == 0, result

            snapshot = await call_api(
                create_snapshot, client=c, vm_id=vm_id, body=CreateSnapshotRequest()
            )
            assert snapshot.status == SnapshotStatus.READY

            result = await exec_when_ready(c, vm_id, _write_marker("after!"))
            assert result.exit_code == 0, result
            await call_api(stop_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.SHUTDOWN, timeout=VM_OPERATION_TIMEOUT)

            restored = await call_api(
                restore, client=c, vm_id=vm_id, body=RestoreRequest(snapshot_id=snapshot.id)
            )
            assert restored.status == VmStatus.RUNNING
            result = await exec_when_ready(
                c,
                vm_id,
                "echo 3 > /proc/sys/vm/drop_caches; busybox dd if=/dev/vda bs=6 count=1 2>/dev/null",
            )
            assert result.exit_code == 0, result
            assert result.stdout == "before", result.stdout


@pytest.mark.asyncio
async def test_a_vm_with_a_qcow2_disk_is_not_snapshotted(client):
    """
    Cloud Hypervisor keeps a qcow2 disk's metadata in memory and writes it to
    the image when the guest flushes the disk, so a copy taken while the VM is
    paused could miss what the guest wrote. Such a snapshot is refused, not
    taken without the disk, and the VM keeps running.
    """
    async with client as c:
        async with _running_vm_with_disk(c, Hypervisor.CLOUD_HV, "qcow2") as vm_id:
            resp = await call_api_detailed(
                create_snapshot, client=c, vm_id=vm_id, body=CreateSnapshotRequest()
            )
            assert resp.status_code == 422, resp.content.decode()
            assert "qcow2" in resp.content.decode(), resp.content.decode()
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING
