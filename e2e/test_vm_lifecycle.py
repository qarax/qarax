"""
E2E tests for VM lifecycle using qarax-api-client SDK

These tests verify the full VM lifecycle with real Cloud Hypervisor VMs:
- Creating a VM
- Starting the VM (boots with test kernel/initramfs)
- Pausing the VM
- Resuming the VM
- Stopping the VM
- Deleting the VM
"""

import asyncio
import os
import time

import pytest
from qarax_api_client import Client
from qarax_api_client.api.vms import (
    list_ as list_vms,
    create as create_vm,
    get as get_vm,
    start as start_vm,
    stop as stop_vm,
    force_stop as force_stop_vm,
    pause as pause_vm,
    resume as resume_vm,
    delete as delete_vm,
)
from qarax_api_client.models import NewVm, Hypervisor, VmStatus


# Base URL for the qarax API (can be overridden via environment variable)
QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")

# Timeout for VM operations (real VMs take time to boot/stop)
VM_OPERATION_TIMEOUT = 30


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL)


async def call_api(endpoint_module, **kwargs):
    """
    Call generated SDK endpoint in a version-tolerant way.

    Some generated endpoints expose only `asyncio_detailed` (no `asyncio`) when
    the operation has no parsed response body.
    """
    asyncio_fn = getattr(endpoint_module, "asyncio", None)
    if callable(asyncio_fn):
        return await asyncio_fn(**kwargs)

    detailed_fn = getattr(endpoint_module, "asyncio_detailed", None)
    if callable(detailed_fn):
        response = await detailed_fn(**kwargs)
        return response.parsed

    raise AttributeError(f"{endpoint_module.__name__} has no async entrypoint")


async def wait_for_status(
    client, vm_id: str, expected_status: VmStatus, timeout: int = VM_OPERATION_TIMEOUT
):
    """Wait for a VM to reach the expected status."""
    start_time = time.time()
    while time.time() - start_time < timeout:
        vm = await call_api(get_vm, client=client, vm_id=vm_id)
        if vm.status == expected_status:
            return vm
        await asyncio.sleep(0.5)

    # Get final state for error message
    vm = await call_api(get_vm, client=client, vm_id=vm_id)
    raise TimeoutError(
        f"VM {vm_id} did not reach status {expected_status} within {timeout}s. Current status: {vm.status}"
    )


@pytest.mark.asyncio
async def test_vm_create_and_list(client):
    """Test creating a VM and listing VMs."""
    async with client as c:
        # Create a new VM
        new_vm = NewVm(
            name="test-vm-e2e-create",
            tags=["e2e", "smoke"],
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,  # 256MB - minimal for test kernel
        )

        vm_id = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id is not None

        try:
            # Verify VM was created
            vm = await call_api(get_vm, client=c, vm_id=str(vm_id))
            assert vm.name == "test-vm-e2e-create"
            assert vm.tags == ["e2e", "smoke"]
            assert vm.status == VmStatus.CREATED
            assert vm.boot_vcpus == 1
            assert vm.max_vcpus == 1
            assert vm.memory_size == 256 * 1024 * 1024

            # List VMs and verify our VM is in the list
            vms = await call_api(list_vms, client=c)
            assert vms is not None
            listed_vm = next(v for v in vms if v.id == vm.id)
            assert listed_vm.tags == ["e2e", "smoke"]

        finally:
            # Cleanup
            await call_api(delete_vm, client=c, vm_id=str(vm_id))


@pytest.mark.asyncio
async def test_vm_list_filter_by_tags(client):
    """Test that GET /vms?tags=... returns only VMs that have all specified tags."""
    async with client as c:
        # Create two VMs with different tag sets
        vm_a = NewVm(
            name="test-vm-tags-a",
            tags=["env:test", "team:platform"],
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        vm_b = NewVm(
            name="test-vm-tags-b",
            tags=["env:test"],
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )

        id_a = await call_api(create_vm, client=c, body=vm_a)
        id_b = await call_api(create_vm, client=c, body=vm_b)

        try:
            str_a, str_b = str(id_a), str(id_b)

            # Filter by single tag shared by both — both should appear
            vms = await call_api(list_vms, client=c, tags="env:test")
            assert vms is not None
            ids = {str(v.id) for v in vms}
            assert str_a in ids
            assert str_b in ids

            # Filter by both tags — only vm_a should appear
            vms = await call_api(list_vms, client=c, tags="env:test,team:platform")
            assert vms is not None
            ids = {str(v.id) for v in vms}
            assert str_a in ids
            assert str_b not in ids

            # Filter by a tag neither VM has — empty result (for our VMs at least)
            vms = await call_api(list_vms, client=c, tags="env:prod")
            assert vms is not None
            ids = {str(v.id) for v in vms}
            assert str_a not in ids
            assert str_b not in ids

        finally:
            await call_api(delete_vm, client=c, vm_id=str(id_a))
            await call_api(delete_vm, client=c, vm_id=str(id_b))


@pytest.mark.asyncio
async def test_vm_full_lifecycle(client):
    """Test the complete VM lifecycle with real Cloud Hypervisor VMs."""
    async with client as c:
        # 1. Create VM
        new_vm = NewVm(
            name="test-vm-e2e-lifecycle",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,  # 256MB
        )

        vm_id = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id is not None
        vm_id_str = str(vm_id)

        try:
            # Verify initial status
            vm = await call_api(get_vm, client=c, vm_id=vm_id_str)
            assert vm.status == VmStatus.CREATED

            # 2. Start VM (async — returns 202 with job_id, VM reaches RUNNING in background)
            await call_api(start_vm, client=c, vm_id=vm_id_str)
            vm = await wait_for_status(c, vm_id_str, VmStatus.RUNNING)

            # 3. Pause VM
            await call_api(pause_vm, client=c, vm_id=vm_id_str)
            vm = await call_api(get_vm, client=c, vm_id=vm_id_str)
            assert vm.status == VmStatus.PAUSED

            # 4. Resume VM
            await call_api(resume_vm, client=c, vm_id=vm_id_str)
            vm = await call_api(get_vm, client=c, vm_id=vm_id_str)
            assert vm.status == VmStatus.RUNNING

            # 5. Stop VM
            await call_api(stop_vm, client=c, vm_id=vm_id_str)
            vm = await call_api(get_vm, client=c, vm_id=vm_id_str)
            assert vm.status == VmStatus.SHUTDOWN

        finally:
            # 6. Delete VM (cleanup)
            await call_api(delete_vm, client=c, vm_id=vm_id_str)

        # Verify VM is deleted (should not be in list)
        vms = await call_api(list_vms, client=c)
        if vms:
            assert not any(str(v.id) == vm_id_str for v in vms)


@pytest.mark.asyncio
async def test_vm_delete(client):
    """Test VM deletion."""
    async with client as c:
        # Create a VM
        new_vm = NewVm(
            name="test-vm-e2e-delete",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,  # 256MB
        )

        vm_id = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id is not None

        # Delete the VM
        await call_api(delete_vm, client=c, vm_id=str(vm_id))

        # Verify VM is deleted
        vms = await call_api(list_vms, client=c)
        if vms:
            assert not any(str(v.id) == str(vm_id) for v in vms)


@pytest.mark.asyncio
async def test_multiple_vms(client):
    """Test creating and managing multiple VMs."""
    async with client as c:
        vm_ids = []

        try:
            # Create 3 VMs
            for i in range(3):
                new_vm = NewVm(
                    name=f"test-vm-multi-{i}",
                    hypervisor=Hypervisor.CLOUD_HV,
                    boot_vcpus=1,
                    max_vcpus=1,
                    memory_size=256 * 1024 * 1024,  # 256MB each
                )

                vm_id = await call_api(create_vm, client=c, body=new_vm)
                vm_ids.append(str(vm_id))

            # Verify all VMs were created
            vms = await call_api(list_vms, client=c)
            assert vms is not None
            created_vm_ids = [str(v.id) for v in vms]
            for vm_id in vm_ids:
                assert vm_id in created_vm_ids

        finally:
            # Cleanup all VMs
            for vm_id in vm_ids:
                try:
                    await call_api(delete_vm, client=c, vm_id=vm_id)
                except Exception:
                    pass  # Best effort cleanup


@pytest.mark.asyncio
async def test_vm_start_stop_cycle(client):
    """Test starting and stopping a VM multiple times."""
    async with client as c:
        # Create VM
        new_vm = NewVm(
            name="test-vm-e2e-start-stop",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )

        vm_id = await call_api(create_vm, client=c, body=new_vm)
        vm_id_str = str(vm_id)

        try:
            # Start/stop cycle
            for i in range(2):
                # Start (async — polls until RUNNING)
                await call_api(start_vm, client=c, vm_id=vm_id_str)
                vm = await wait_for_status(c, vm_id_str, VmStatus.RUNNING)

                # Small delay
                await asyncio.sleep(0.5)

                # Stop
                await call_api(stop_vm, client=c, vm_id=vm_id_str)
                vm = await call_api(get_vm, client=c, vm_id=vm_id_str)
                assert vm.status == VmStatus.SHUTDOWN, (
                    f"Cycle {i}: Expected SHUTDOWN after stop"
                )

        finally:
            await call_api(delete_vm, client=c, vm_id=vm_id_str)


@pytest.mark.asyncio
async def test_vm_force_stop(client):
    """Test force stopping (hard power-off) a running VM."""
    async with client as c:
        # Create VM
        new_vm = NewVm(
            name="test-vm-e2e-force-stop",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )

        vm_id = await call_api(create_vm, client=c, body=new_vm)
        vm_id_str = str(vm_id)

        try:
            # Start VM and wait for it to be running
            await call_api(start_vm, client=c, vm_id=vm_id_str)
            await wait_for_status(c, vm_id_str, VmStatus.RUNNING)

            # Force stop the VM
            await call_api(force_stop_vm, client=c, vm_id=vm_id_str)
            vm = await call_api(get_vm, client=c, vm_id=vm_id_str)
            assert vm.status == VmStatus.SHUTDOWN

        finally:
            await call_api(delete_vm, client=c, vm_id=vm_id_str)
