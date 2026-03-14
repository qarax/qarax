"""
E2E tests for VM snapshot support.

These tests verify snapshot lifecycle against a real Cloud Hypervisor VM:
- Creating a snapshot of a running VM (pause → snapshot → resume)
- Listing snapshots for a VM
- Snapshot record persists with status 'ready' after success
- Listing snapshots for an unknown VM returns 404
"""

import os
import time
import uuid

import pytest
from qarax_api_client import Client
from qarax_api_client.api.vms import (
    create as create_vm,
    create_snapshot,
    delete as delete_vm,
    get as get_vm,
    list_snapshots,
    start as start_vm,
    stop as stop_vm,
)
from qarax_api_client.models import Hypervisor, NewVm, VmStatus
from qarax_api_client.models.snapshot_status import SnapshotStatus

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
VM_OPERATION_TIMEOUT = 60


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


async def wait_for_status(client, vm_id: str, expected_status: VmStatus, timeout: int = VM_OPERATION_TIMEOUT):
    import asyncio
    start = time.time()
    while time.time() - start < timeout:
        vm = await get_vm.asyncio(client=client, vm_id=uuid.UUID(vm_id))
        if vm.status == expected_status:
            return vm
        await asyncio.sleep(0.5)
    vm = await get_vm.asyncio(client=client, vm_id=uuid.UUID(vm_id))
    raise TimeoutError(
        f"VM {vm_id} did not reach {expected_status} within {timeout}s. Current: {vm.status}"
    )


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
        resp = await create_snapshot.asyncio_detailed(client=c, vm_id=unknown_id)
        assert resp.status_code == 404, f"Expected 404, got {resp.status_code}"


@pytest.mark.asyncio
async def test_snapshot_full_lifecycle(client):
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
            await wait_for_status(c, str(vm_id), VmStatus.RUNNING)

            # Create a snapshot
            snapshot = await create_snapshot.asyncio(client=c, vm_id=vm_id)
            assert snapshot is not None, "Expected snapshot object, got None"
            assert snapshot.vm_id == vm_id
            assert snapshot.status == SnapshotStatus.READY
            assert snapshot.snapshot_url.startswith("file://")

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
