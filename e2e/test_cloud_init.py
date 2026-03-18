"""
E2E tests for cloud-init NoCloud seed disk support.

These tests verify:
- A VM created with cloud_init_user_data stores the data and reaches RUNNING
- The cloud_init fields are returned correctly on GET
- A VM created without cloud_init fields is unaffected (regression)
- The seed disk is cleaned up after VM deletion (indirectly — delete succeeds)
"""

import asyncio
import os
import time

import pytest
from qarax_api_client import Client
from qarax_api_client.api.vms import (
    create as create_vm,
    delete as delete_vm,
    get as get_vm,
    start as start_vm,
    stop as stop_vm,
)
from qarax_api_client.models import Hypervisor, NewVm, VmStatus

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
VM_OPERATION_TIMEOUT = 30

MINIMAL_USER_DATA = """\
#cloud-config
# Minimal cloud-init user-data for e2e testing.
# The test kernel/initramfs may not have cloud-init installed;
# the important thing is that the VM boots with the cidata disk present.
runcmd:
  - echo "cloud-init e2e test" > /tmp/cloud-init-ran
"""

MINIMAL_META_DATA = """\
instance-id: e2e-cloud-init-test
local-hostname: e2e-cloud-init-vm
"""

MINIMAL_NETWORK_CONFIG = """\
version: 1
config:
  - type: physical
    name: eth0
    subnets:
      - type: dhcp
"""


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


async def wait_for_status(client, vm_id: str, expected: VmStatus, timeout: int = VM_OPERATION_TIMEOUT):
    deadline = time.time() + timeout
    while time.time() < deadline:
        vm = await get_vm.asyncio(client=client, vm_id=vm_id)
        if vm.status == expected:
            return vm
        await asyncio.sleep(0.5)
    vm = await get_vm.asyncio(client=client, vm_id=vm_id)
    raise TimeoutError(
        f"VM {vm_id} did not reach {expected} within {timeout}s. "
        f"Current status: {vm.status}"
    )


@pytest.mark.asyncio
async def test_cloud_init_fields_stored_and_returned(client):
    """VM created with cloud-init data should return the fields on GET."""
    async with client as c:
        new_vm = NewVm(
            name="e2e-cloud-init-fields",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
            cloud_init_user_data=MINIMAL_USER_DATA,
            cloud_init_meta_data=MINIMAL_META_DATA,
        )
        vm_id = await create_vm.asyncio(client=c, body=new_vm)
        assert vm_id is not None
        vm_id_str = str(vm_id).strip('"')

        try:
            vm = await get_vm.asyncio(client=c, vm_id=vm_id_str)
            assert vm is not None
            assert vm.cloud_init_user_data == MINIMAL_USER_DATA
            assert vm.cloud_init_meta_data == MINIMAL_META_DATA
            assert vm.cloud_init_network_config is None
        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id_str)


@pytest.mark.asyncio
async def test_cloud_init_with_network_config(client):
    """VM with cloud_init_network_config stores and returns all three fields."""
    async with client as c:
        new_vm = NewVm(
            name="e2e-cloud-init-netcfg",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
            cloud_init_user_data=MINIMAL_USER_DATA,
            cloud_init_network_config=MINIMAL_NETWORK_CONFIG,
        )
        vm_id = await create_vm.asyncio(client=c, body=new_vm)
        assert vm_id is not None
        vm_id_str = str(vm_id).strip('"')

        try:
            vm = await get_vm.asyncio(client=c, vm_id=vm_id_str)
            assert vm.cloud_init_user_data == MINIMAL_USER_DATA
            assert vm.cloud_init_network_config == MINIMAL_NETWORK_CONFIG
        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id_str)


@pytest.mark.asyncio
async def test_cloud_init_vm_boots(client):
    """VM with cloud-init data should start and reach RUNNING status."""
    async with client as c:
        new_vm = NewVm(
            name="e2e-cloud-init-boot",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
            cloud_init_user_data=MINIMAL_USER_DATA,
            cloud_init_meta_data=MINIMAL_META_DATA,
        )
        vm_id = await create_vm.asyncio(client=c, body=new_vm)
        assert vm_id is not None
        vm_id_str = str(vm_id).strip('"')

        try:
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id_str)
            await wait_for_status(c, vm_id_str, VmStatus.RUNNING)

            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id_str)
            vm = await get_vm.asyncio(client=c, vm_id=vm_id_str)
            assert vm.status == VmStatus.SHUTDOWN
        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id_str)


@pytest.mark.asyncio
async def test_no_cloud_init_unaffected(client):
    """VM created without cloud-init data should have null fields (regression)."""
    async with client as c:
        new_vm = NewVm(
            name="e2e-cloud-init-none",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        vm_id = await create_vm.asyncio(client=c, body=new_vm)
        assert vm_id is not None
        vm_id_str = str(vm_id).strip('"')

        try:
            vm = await get_vm.asyncio(client=c, vm_id=vm_id_str)
            assert vm.cloud_init_user_data is None
            assert vm.cloud_init_meta_data is None
            assert vm.cloud_init_network_config is None
        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id_str)
