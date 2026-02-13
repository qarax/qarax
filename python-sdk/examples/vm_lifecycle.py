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
from qarax_api_client.models import NewVm, Hypervisor


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

        vm_id = await create_vm.asyncio(client=c, body=new_vm)
        print(f"Created VM with ID: {vm_id}")

        print("\n=== Getting VM details ===")
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        print(f"VM: {vm.name} (Status: {vm.status})")
        print(f"  CPU: {vm.boot_vcpus} vCPUs (max: {vm.max_vcpus})")
        print(f"  Memory: {vm.memory_size / (1024**3):.2f} GB")

        print("\n=== Starting VM ===")
        await start_vm.asyncio(client=c, vm_id=str(vm_id))
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        print(f"VM Status: {vm.status}")

        print("\n=== Pausing VM ===")
        await pause_vm.asyncio(client=c, vm_id=str(vm_id))
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        print(f"VM Status: {vm.status}")

        print("\n=== Resuming VM ===")
        await resume_vm.asyncio(client=c, vm_id=str(vm_id))
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        print(f"VM Status: {vm.status}")

        print("\n=== Stopping VM ===")
        await stop_vm.asyncio(client=c, vm_id=str(vm_id))
        vm = await get_vm.asyncio(client=c, vm_id=str(vm_id))
        print(f"VM Status: {vm.status}")

        print("\n=== Deleting VM ===")
        await delete_vm.asyncio(client=c, vm_id=str(vm_id))
        print(f"VM {vm_id} deleted successfully")

        print("\n=== Listing all VMs ===")
        vms = await list_vms.asyncio(client=c)
        print(f"Total VMs: {len(vms) if vms else 0}")


if __name__ == "__main__":
    asyncio.run(demonstrate_vm_lifecycle())
