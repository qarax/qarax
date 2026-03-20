"""
E2E tests for CPU and memory hotplug (VM resize).

Covers:
  - Resize vCPUs on a running VM
  - Resize memory on a running VM
  - Resize both vCPUs and memory in one call
  - Negative: resize a Created (not running) VM → 422
  - Negative: desired_vcpus out of range → 422
  - Negative: desired_ram out of range → 422
  - Negative: neither field provided → 422

Prerequisites for running-VM tests:
  - A qarax-node instance reachable with KVM passthrough
  - VM must be created with max_vcpus > 1 and memory_hotplug_size > 0
"""

import asyncio
import os
import time
import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.vms import (
    create as create_vm,
    delete as delete_vm,
    get as get_vm,
    resize_vm,
    start as start_vm,
    stop as stop_vm,
)
from qarax_api_client.models import Hypervisor, NewVm, VmStatus
from qarax_api_client.models.vm_resize_request import VmResizeRequest

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
VM_OPERATION_TIMEOUT = 30

# VM config for resize tests: 1 boot vCPU, 4 max vCPUs, 256 MiB base + 256 MiB hotplug
BOOT_VCPUS = 1
MAX_VCPUS = 4
MEMORY_SIZE = 256 * 1024 * 1024
MEMORY_HOTPLUG_SIZE = 256 * 1024 * 1024


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


async def wait_for_status(c, vm_id, expected_status, timeout=VM_OPERATION_TIMEOUT):
    start = time.time()
    while time.time() - start < timeout:
        vm = await get_vm.asyncio(client=c, vm_id=vm_id)
        if vm.status == expected_status:
            return vm
        await asyncio.sleep(0.5)
    vm = await get_vm.asyncio(client=c, vm_id=vm_id)
    raise TimeoutError(
        f"VM {vm_id} did not reach {expected_status} within {timeout}s. Current: {vm.status}"
    )


async def _make_resize_vm(c, test_id):
    """Create a VM with hotplug capacity and return its UUID."""
    vm_id_raw = await create_vm.asyncio(
        client=c,
        body=NewVm(
            name=f"e2e-resize-vm-{test_id}",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=BOOT_VCPUS,
            max_vcpus=MAX_VCPUS,
            memory_size=MEMORY_SIZE,
            memory_hotplug_size=MEMORY_HOTPLUG_SIZE,
        ),
    )
    return UUID(str(vm_id_raw).strip('"'))


# ─── Negative tests (no boot required) ────────────────────────────────────────


@pytest.mark.asyncio
async def test_resize_created_vm_returns_422(client):
    """Resizing a Created (not yet started) VM returns 422."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        vm_id = None
        try:
            vm_id = await _make_resize_vm(c, test_id)
            resp = await resize_vm.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmResizeRequest(desired_vcpus=2),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for Created VM, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_resize_no_fields_returns_422(client):
    """Sending an empty resize request (no fields) returns 422."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        vm_id = None
        try:
            vm_id = await _make_resize_vm(c, test_id)
            resp = await resize_vm.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmResizeRequest(),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for empty resize, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_resize_vcpus_out_of_range_returns_422(client):
    """Requesting more vCPUs than max_vcpus returns 422."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        vm_id = None
        try:
            vm_id = await _make_resize_vm(c, test_id)
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            resp = await resize_vm.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmResizeRequest(desired_vcpus=MAX_VCPUS + 1),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for vcpus > max_vcpus, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
                await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_resize_ram_out_of_range_returns_422(client):
    """Requesting more RAM than memory_size + hotplug_size returns 422."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        vm_id = None
        try:
            vm_id = await _make_resize_vm(c, test_id)
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            over_limit = MEMORY_SIZE + MEMORY_HOTPLUG_SIZE + 1
            resp = await resize_vm.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmResizeRequest(desired_ram=over_limit),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for ram > max, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
                await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


# ─── Running-VM resize tests (require real Cloud Hypervisor) ──────────────────


@pytest.mark.asyncio
async def test_resize_vcpus_running_vm(client):
    """
    Hotplug 2 additional vCPUs into a running VM, verify the response reflects
    the new count and the VM stays Running.
    """
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        vm_id = None
        try:
            vm_id = await _make_resize_vm(c, test_id)
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            resp = await resize_vm.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmResizeRequest(desired_vcpus=2),
            )
            assert resp.status_code == 200, (
                f"CPU resize failed: {resp.status_code} {resp.content.decode()}"
            )
            updated = resp.parsed
            assert updated.boot_vcpus == 2, (
                f"Expected boot_vcpus=2, got {updated.boot_vcpus}"
            )

            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, "VM should still be running after CPU resize"
            assert vm.boot_vcpus == 2

            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_resize_memory_running_vm(client):
    """
    Hotplug additional memory into a running VM, verify the response reflects
    the new size and the VM stays Running.
    """
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        vm_id = None
        try:
            vm_id = await _make_resize_vm(c, test_id)
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            new_ram = MEMORY_SIZE + 128 * 1024 * 1024  # +128 MiB
            resp = await resize_vm.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmResizeRequest(desired_ram=new_ram),
            )
            assert resp.status_code == 200, (
                f"Memory resize failed: {resp.status_code} {resp.content.decode()}"
            )
            updated = resp.parsed
            assert updated.memory_size == new_ram, (
                f"Expected memory_size={new_ram}, got {updated.memory_size}"
            )

            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, "VM should still be running after memory resize"
            assert vm.memory_size == new_ram

            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_resize_memory_invalid_hotplug_increment_returns_422(client):
    """Requesting a non-128 MiB hotplug increment returns 422."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        vm_id = None
        try:
            vm_id = await _make_resize_vm(c, test_id)
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            invalid_ram = MEMORY_SIZE + 64 * 1024 * 1024  # +64 MiB is not a valid ACPI hotplug step
            resp = await resize_vm.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmResizeRequest(desired_ram=invalid_ram),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for invalid hotplug increment, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
                await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_resize_vcpus_and_memory_running_vm(client):
    """
    Resize both vCPUs and memory in a single call; verify both fields are updated.
    """
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        vm_id = None
        try:
            vm_id = await _make_resize_vm(c, test_id)
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            new_ram = MEMORY_SIZE + 128 * 1024 * 1024  # +128 MiB
            resp = await resize_vm.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmResizeRequest(desired_vcpus=3, desired_ram=new_ram),
            )
            assert resp.status_code == 200, (
                f"Combined resize failed: {resp.status_code} {resp.content.decode()}"
            )
            updated = resp.parsed
            assert updated.boot_vcpus == 3
            assert updated.memory_size == new_ram

            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING
            assert vm.boot_vcpus == 3
            assert vm.memory_size == new_ram

            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
