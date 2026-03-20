"""
E2E tests for GPU-aware scheduling and host GPU inventory.

These tests verify:
- Listing GPUs on a host (empty list when no VFIO GPUs are bound)
- Creating instance types with accelerator_config
- VM creation with GPU requirements (expected to fail gracefully when no GPUs available)
"""

import os

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.instance_types import (
    create as create_instance_type,
    delete as delete_instance_type,
    get as get_instance_type,
)
from qarax_api_client.api.vms import (
    create as create_vm,
)
from qarax_api_client.models import NewInstanceType, NewVm, Hypervisor
import httpx


QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL)


@pytest.fixture
def http_client():
    """Create a raw HTTP client for endpoints not yet in the generated SDK."""
    return httpx.Client(base_url=QARAX_URL)


def get_first_host_id(client):
    """Get the first host's ID from the API."""
    hosts = list_hosts.sync(client=client)
    assert hosts is not None and len(hosts) > 0, "No hosts registered"
    return hosts[0].id


@pytest.mark.asyncio
async def test_list_host_gpus(client, http_client):
    """Test listing GPUs on a host returns a valid (possibly empty) list."""
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts is not None and len(hosts) > 0

        host_id = hosts[0].id

    # Use raw HTTP since the SDK may not have the /gpus endpoint yet
    resp = http_client.get(f"/hosts/{host_id}/gpus")
    assert resp.status_code == 200
    gpus = resp.json()
    assert isinstance(gpus, list)
    # In CI/e2e there are typically no VFIO-bound GPUs, so empty is expected
    for gpu in gpus:
        assert "id" in gpu
        assert "pci_address" in gpu
        assert "iommu_group" in gpu


@pytest.mark.asyncio
async def test_instance_type_with_accelerator_config(client):
    """Test creating an instance type with accelerator_config for GPU requirements."""
    async with client as c:
        new_it = NewInstanceType(
            name="test-gpu-instance-type",
            boot_vcpus=2,
            max_vcpus=2,
            memory_size=512 * 1024 * 1024,
            accelerator_config={"gpu_count": 1, "gpu_vendor": "nvidia"},
        )

        it_id = await create_instance_type.asyncio(client=c, body=new_it)
        assert it_id is not None

        try:
            it = await get_instance_type.asyncio(
                client=c, instance_type_id=str(it_id)
            )
            assert it is not None
            assert it.name == "test-gpu-instance-type"
        finally:
            await delete_instance_type.asyncio_detailed(
                client=c, instance_type_id=str(it_id)
            )


@pytest.mark.asyncio
async def test_vm_create_with_gpu_no_available_gpus(client):
    """
    Test that creating a VM with GPU requirements fails gracefully
    when no GPUs are available on any host.
    """
    async with client as c:
        # Create a VM requesting 1 GPU - should fail since e2e hosts have no VFIO GPUs
        new_vm = NewVm(
            name="test-vm-gpu-fail",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
            accelerator_config={"gpu_count": 1, "gpu_vendor": "nvidia"},
        )

        # The API should return an error (422 or similar) since no host has GPUs
        result = await create_vm.asyncio_detailed(client=c, body=new_vm)
        # We expect failure — either 422 (no suitable host) or a different error code
        assert result.status_code.value != 201, (
            "VM creation should have failed when no GPUs are available"
        )
