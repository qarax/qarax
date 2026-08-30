"""
E2E tests for VM snapshot support.

These tests verify snapshot lifecycle against a real Cloud Hypervisor VM:
- Creating a snapshot of a running VM (pause → snapshot → resume)
- Listing snapshots for a VM
- Snapshot record persists with status 'ready' after success
- Listing snapshots for an unknown VM returns 404
"""

import uuid

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_pools import attach_host as attach_pool_host
from qarax_api_client.api.storage_pools import create as create_pool
from qarax_api_client.api.storage_pools import delete as delete_pool
from qarax_api_client.api.vms import (
    create as create_vm,
)
from qarax_api_client.api.vms import (
    create_snapshot,
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
from qarax_api_client.models import HostStatus, Hypervisor, NewStoragePool, NewVm, StoragePoolType, VmStatus
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest
from qarax_api_client.models.create_snapshot_request import CreateSnapshotRequest
from qarax_api_client.models.restore_request import RestoreRequest
from qarax_api_client.models.snapshot_status import SnapshotStatus

VM_OPERATION_TIMEOUT = 60

from helpers import AUTH_HEADERS, QARAX_URL, wait_for_status


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


@pytest.fixture(scope="module")
def snapshot_storage_pool():
    """Create a local storage pool for snapshot tests and attach it to the host."""
    with Client(base_url=QARAX_URL, headers=AUTH_HEADERS) as c:
        hosts = [h for h in (list_hosts.sync(client=c) or []) if h.status == HostStatus.UP]
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
async def test_snapshot_list_empty_for_new_vm(client):
    """A freshly created VM should have no snapshots."""
    async with client as c:
        new_vm = NewVm(
            name="e2e-snap-list-empty",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        result = await create_vm.asyncio(client=c, body=new_vm)
        vm_id = uuid.UUID(str(result).strip('"'))

        try:
            snapshots = await list_snapshots.asyncio(client=c, vm_id=vm_id)
            assert snapshots == [], f"Expected empty list, got: {snapshots}"
        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_snapshot_list_unknown_vm_returns_404(client):
    """Listing snapshots for an unknown VM returns 404."""
    async with client as c:
        unknown_id = uuid.uuid4()
        resp = await list_snapshots.asyncio_detailed(client=c, vm_id=unknown_id)
        assert resp.status_code == 404, f"Expected 404, got {resp.status_code}"


@pytest.mark.asyncio
async def test_create_snapshot_unknown_vm_returns_404(client):
    """Creating a snapshot for an unknown VM returns 404."""
    async with client as c:
        unknown_id = uuid.uuid4()
        resp = await create_snapshot.asyncio_detailed(client=c, vm_id=unknown_id, body=CreateSnapshotRequest())
        assert resp.status_code == 404, f"Expected 404, got {resp.status_code}"


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
