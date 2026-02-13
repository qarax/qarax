"""
Basic usage example for the qarax-api-client SDK

This example demonstrates how to interact with the Qarax API
to manage hosts and virtual machines.
"""
import asyncio
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.vms import list_ as list_vms


async def main():
    """Main example function demonstrating the SDK usage."""
    # Create a client (adjust base_url to match your qarax server)
    client = Client(base_url="http://localhost:8000")

    async with client as c:
        print("=== Listing Hosts ===")
        hosts = await list_hosts.asyncio(client=c)
        if hosts:
            for host in hosts:
                print(f"Host: {host.name} ({host.address}:{host.port}) - Status: {host.status}")
        else:
            print("No hosts found")

        print("\n=== Listing VMs ===")
        vms = await list_vms.asyncio(client=c)
        if vms:
            for vm in vms:
                print(f"VM: {vm.name} (ID: {vm.id}) - Status: {vm.status}")
                print(f"  CPU: {vm.boot_vcpus} vCPUs, Memory: {vm.memory_size} bytes")
                print(f"  Hypervisor: {vm.hypervisor}")
        else:
            print("No VMs found")


def sync_example():
    """Synchronous example for comparison."""
    client = Client(base_url="http://localhost:8000")

    with client as c:
        print("\n=== Synchronous Host Listing ===")
        hosts = list_hosts.sync(client=c)
        if hosts:
            for host in hosts:
                print(f"Host: {host.name} - Status: {host.status}")
        else:
            print("No hosts found")


if __name__ == "__main__":
    # Run async example
    asyncio.run(main())

    # Run sync example
    sync_example()
