"""
VM Lifecycle example for the qarax-api-client SDK

This example demonstrates the full VM lifecycle:
- Creating a VM
- Starting the VM
- Pausing the VM
- Resuming the VM
- Stopping the VM
- Deleting the VM
"""

import asyncio
from uuid import UUID

from qarax_api_client import Client
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.api.vms import list_ as list_vms
from qarax_api_client.api.vms import pause as pause_vm
from qarax_api_client.api.vms import resume as resume_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.api.vms import stop as stop_vm
from qarax_api_client.models import Hypervisor, NewVm


async def demonstrate_vm_lifecycle():
    """Demonstrate the full VM lifecycle using the SDK."""
    client = Client(base_url="http://localhost:8000")

    async with client as c:
        print("=== Creating a new VM ===")
        new_vm = NewVm(
            name="test-vm-sdk",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=2,
            max_vcpus=4,
            memory_size=2 * 1024 * 1024 * 1024,  # 2GB in bytes
        )

        vm_id_str = await create_vm.asyncio(client=c, body=new_vm)
        assert isinstance(vm_id_str, str), f"Expected VM ID string, got {vm_id_str!r}"
        vm_id = UUID(vm_id_str)
        print(f"Created VM with ID: {vm_id}")

        print("\n=== Getting VM details ===")
        vm = await get_vm.asyncio(vm_id=vm_id, client=c)
        assert vm is not None and not isinstance(vm, dict), "Failed to retrieve VM"
        print(f"VM: {vm.name} (Status: {vm.status})")
        print(f"  CPU: {vm.boot_vcpus} vCPUs (max: {vm.max_vcpus})")
        print(f"  Memory: {vm.memory_size / (1024**3):.2f} GB")

        print("\n=== Starting VM ===")
        await start_vm.asyncio_detailed(vm_id=vm_id, client=c)
        vm = await get_vm.asyncio(vm_id=vm_id, client=c)
        assert vm is not None and not isinstance(vm, dict), "Failed to retrieve VM"
        print(f"VM Status: {vm.status}")

        print("\n=== Pausing VM ===")
        await pause_vm.asyncio_detailed(vm_id=vm_id, client=c)
        vm = await get_vm.asyncio(vm_id=vm_id, client=c)
        assert vm is not None and not isinstance(vm, dict), "Failed to retrieve VM"
        print(f"VM Status: {vm.status}")

        print("\n=== Resuming VM ===")
        await resume_vm.asyncio_detailed(vm_id=vm_id, client=c)
        vm = await get_vm.asyncio(vm_id=vm_id, client=c)
        assert vm is not None and not isinstance(vm, dict), "Failed to retrieve VM"
        print(f"VM Status: {vm.status}")

        print("\n=== Stopping VM ===")
        await stop_vm.asyncio_detailed(vm_id=vm_id, client=c)
        vm = await get_vm.asyncio(vm_id=vm_id, client=c)
        assert vm is not None and not isinstance(vm, dict), "Failed to retrieve VM"
        print(f"VM Status: {vm.status}")

        print("\n=== Deleting VM ===")
        await delete_vm.asyncio_detailed(vm_id=vm_id, client=c)
        print(f"VM {vm_id} deleted successfully")

        print("\n=== Listing VMs ===")
        vms = await list_vms.asyncio(client=c)
        print(f"VMs on the first page: {len(vms.items) if vms else 0}")


if __name__ == "__main__":
    asyncio.run(demonstrate_vm_lifecycle())
