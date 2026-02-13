"""
E2E tests for VM lifecycle using qarax-api-client SDK

These tests verify the full VM lifecycle:
- Creating a VM
- Starting the VM
- Pausing the VM
- Resuming the VM
- Stopping the VM
- Deleting the VM
"""
import os

import pytest
from qarax_api_client import Client
from qarax_api_client.api.vms import (
    list_ as list_vms,
    create as create_vm,
    get as get_vm,
    start as start_vm,
    stop as stop_vm,
    pause as pause_vm,
    resume as resume_vm,
    delete as delete_vm,
)
from qarax_api_client.models import NewVm, Hypervisor, VmStatus


# Base URL for the qarax API (can be overridden via environment variable)
QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL)


@pytest.mark.asyncio
async def test_vm_create_and_list(client):
    """Test creating a VM and listing VMs."""
    async with client as c:
        # Create a new VM
        new_vm = NewVm(
            name="test-vm-e2e-create",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=2,
            max_vcpus=4,
            memory_size=2 * 1024 * 1024 * 1024,  # 2GB
        )

        vm_id = await create_vm.asyncio(client=c, body=new_vm)
        assert vm_id is not None

        # Verify VM was created
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        assert vm.name == "test-vm-e2e-create"
        assert vm.status == VmStatus.CREATED
        assert vm.boot_vcpus == 2
        assert vm.max_vcpus == 4
        assert vm.memory_size == 2 * 1024 * 1024 * 1024

        # List VMs and verify our VM is in the list
        vms = await list_vms.asyncio(client=c)
        assert vms is not None
        assert any(v.id == vm.id for v in vms)

        # Cleanup
        await delete_vm.asyncio(client=c, vm_id=str(vm_id))


@pytest.mark.asyncio
async def test_vm_full_lifecycle(client):
    """Test the complete VM lifecycle."""
    async with client as c:
        # 1. Create VM
        new_vm = NewVm(
            name="test-vm-e2e-lifecycle",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=2,
            memory_size=1 * 1024 * 1024 * 1024,  # 1GB
        )

        vm_id = await create_vm.asyncio(client=c, body=new_vm)
        assert vm_id is not None

        # Verify initial status
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        assert vm.status == VmStatus.CREATED

        # 2. Start VM
        await start_vm.asyncio(client=c, vm_id=str(vm_id))
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        assert vm.status == VmStatus.RUNNING

        # 3. Pause VM
        await pause_vm.asyncio(client=c, vm_id=str(vm_id))
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        assert vm.status == VmStatus.PAUSED

        # 4. Resume VM
        await resume_vm.asyncio(client=c, vm_id=str(vm_id))
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        assert vm.status == VmStatus.RUNNING

        # 5. Stop VM
        await stop_vm.asyncio(client=c, vm_id=str(vm_id))
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        assert vm.status == VmStatus.SHUTDOWN

        # 6. Delete VM
        await delete_vm.asyncio(client=c, vm_id=str(vm_id))

        # Verify VM is deleted (should raise an error or return None)
        # Note: Depending on the API behavior, this might need adjustment
        vms = await list_vms.asyncio(client=c)
        if vms:
            assert not any(v.id == vm.id for v in vms)


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
            memory_size=512 * 1024 * 1024,  # 512MB
        )

        vm_id = await create_vm.asyncio(client=c, body=new_vm)
        assert vm_id is not None

        # Delete the VM
        await delete_vm.asyncio(client=c, vm_id=str(vm_id))

        # Verify VM is deleted
        vms = await list_vms.asyncio(client=c)
        if vms:
            assert not any(str(v.id) == str(vm_id) for v in vms)


@pytest.mark.asyncio
async def test_multiple_vms(client):
    """Test creating and managing multiple VMs."""
    async with client as c:
        vm_ids = []

        # Create 3 VMs
        for i in range(3):
            new_vm = NewVm(
                name=f"test-vm-multi-{i}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=2,
                memory_size=1 * 1024 * 1024 * 1024,
            )

            vm_id = await create_vm.asyncio(client=c, body=new_vm)
            vm_ids.append(vm_id)

        # Verify all VMs were created
        vms = await list_vms.asyncio(client=c)
        assert vms is not None
        assert len([v for v in vms if str(v.id) in vm_ids]) == 3

        # Cleanup all VMs
        for vm_id in vm_ids:
            await delete_vm.asyncio(client=c, vm_id=str(vm_id))
