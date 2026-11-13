"""
E2E tests for GPU-aware scheduling.

The e2e hosts have no VFIO-bound GPUs, so a VM that requests one must be
rejected by the scheduler instead of landing on a host without GPUs.
"""

import pytest
from qarax_api_client import Client
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.models import Hypervisor, NewVm

from helpers import AUTH_HEADERS, QARAX_URL, call_api, call_api_detailed

# pick_host in qarax/src/handlers/vm/handler.rs: the GPU filter in the
# placement query leaves no eligible host.
NO_ELIGIBLE_HOST_MESSAGE = (
    "no hosts in UP state with sufficient resources available for scheduling"
)


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


@pytest.mark.asyncio
async def test_vm_create_with_gpu_no_available_gpus(client):
    """Creating a VM that requests a GPU fails with 422 when no host has a free GPU."""
    async with client as c:
        new_vm = NewVm(
            name="test-vm-gpu-fail",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
            accelerator_config={"gpu_count": 1, "gpu_vendor": "nvidia"},
        )

        result = await call_api_detailed(create_vm, client=c, body=new_vm)
        try:
            body = result.content.decode(errors="replace")
            assert result.status_code == 422, (
                f"Expected 422 when no GPUs are available, got {result.status_code}: {body}"
            )
            assert NO_ELIGIBLE_HOST_MESSAGE in body, body
        finally:
            if result.status_code == 201:
                vm_id = str(result.parsed).strip('"')
                await call_api(delete_vm, client=c, vm_id=vm_id)
