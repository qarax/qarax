"""
E2E tests for NUMA topology discovery and VM NUMA pinning.

These tests work on single-NUMA machines (the common dev/CI case).
They verify:
  - After host init/probe, at least one NUMA node is discovered and stored
  - Creating a VM with --numa-node 0 (explicit pin) boots successfully
"""

import asyncio
import os
import time

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.hosts import list_numa_nodes
from qarax_api_client.api.vms import (
    create as create_vm,
    delete as delete_vm,
    get as get_vm,
    start as start_vm,
)
from qarax_api_client.models import NewVm, Hypervisor, VmStatus

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
VM_OPERATION_TIMEOUT = 60


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


async def call_api(endpoint_module, **kwargs):
    asyncio_fn = getattr(endpoint_module, "asyncio", None)
    if callable(asyncio_fn):
        return await asyncio_fn(**kwargs)
    detailed_fn = getattr(endpoint_module, "asyncio_detailed", None)
    if callable(detailed_fn):
        response = await detailed_fn(**kwargs)
        return response.parsed
    raise AttributeError(f"{endpoint_module.__name__} has no async entrypoint")


async def wait_for_vm_status(
    client, vm_id: str, expected: VmStatus, timeout: int = VM_OPERATION_TIMEOUT
):
    deadline = time.time() + timeout
    while time.time() < deadline:
        vm = await call_api(get_vm, client=client, vm_id=vm_id)
        if vm.status == expected:
            return vm
        if vm.status in (VmStatus.SHUTDOWN, VmStatus.UNKNOWN):
            raise RuntimeError(
                f"VM {vm_id} entered terminal state {vm.status} waiting for {expected}"
            )
        await asyncio.sleep(1)
    vm = await call_api(get_vm, client=client, vm_id=vm_id)
    raise TimeoutError(
        f"VM {vm_id} did not reach {expected} in {timeout}s; current: {vm.status}"
    )


@pytest.mark.asyncio
async def test_host_numa_topology_discovered(client):
    """After host init the control plane should have at least one NUMA node stored."""
    async with client as c:
        hosts = await call_api(list_hosts, client=c)
        assert hosts, "No hosts registered — run conftest ensure_host_registered first"

        host_id = hosts[0].id
        nodes = await call_api(list_numa_nodes, client=c, host_id=host_id)

        assert nodes is not None, "list_numa_nodes returned None"
        assert len(nodes) >= 1, (
            f"Expected at least one NUMA node for host {host_id}, got {len(nodes)}"
        )

        node0 = next((n for n in nodes if n.node_id == 0), None)
        assert node0 is not None, "NUMA node 0 not found"
        assert node0.cpu_list, f"NUMA node 0 has empty cpu_list: {node0}"


@pytest.mark.asyncio
async def test_vm_start_with_explicit_numa_pin(client):
    """
    A VM created with numa_config={'numa_node': 0} should boot successfully.

    On a single-NUMA machine the pinning maps all vCPUs to node 0 CPUs, which
    is valid and Cloud Hypervisor accepts it without error.
    """
    async with client as c:
        new_vm = NewVm(
            name="test-vm-numa-pin",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
            numa_config={"numa_node": 0},
        )

        vm_id = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id is not None
        vm_id_str = str(vm_id)

        try:
            await call_api(start_vm, client=c, vm_id=vm_id_str)
            vm = await wait_for_vm_status(c, vm_id_str, VmStatus.RUNNING)
            assert vm.status == VmStatus.RUNNING, f"VM status: {vm.status}"
        finally:
            await call_api(delete_vm, client=c, vm_id=vm_id_str)
