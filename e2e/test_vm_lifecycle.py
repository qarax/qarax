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
import time

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.hosts import update as update_host
from qarax_api_client.api.vms import (
    create as create_vm,
)
from qarax_api_client.api.vms import (
    delete as delete_vm,
)
from qarax_api_client.api.vms import (
    exec_ as exec_vm,
)
from qarax_api_client.api.vms import (
    force_stop as force_stop_vm,
)
from qarax_api_client.api.vms import (
    get as get_vm,
)
from qarax_api_client.api.vms import (
    pause as pause_vm,
)
from qarax_api_client.api.vms import (
    resume as resume_vm,
)
from qarax_api_client.api.vms import (
    start as start_vm,
)
from qarax_api_client.api.vms import (
    stop as stop_vm,
)
from qarax_api_client.models import (
    ExecVmRequest,
    HostStatus,
    Hypervisor,
    NewVm,
    UpdateHostRequest,
    VmStatus,
)

VM_OPERATION_TIMEOUT = 30

from helpers import (
    AUTH_HEADERS,
    QARAX_URL,
    call_api,
    call_api_detailed,
    wait_for_status,
)
from test_sandboxes import (
    cleanup_bootable_sandbox_template,
    create_bootable_sandbox_template,
)


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


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
            # 6. Delete VM (also the cleanup path)
            await call_api(delete_vm, client=c, vm_id=vm_id_str)

        resp = await call_api_detailed(get_vm, client=c, vm_id=vm_id_str)
        assert resp.status_code == 404, (
            f"Expected deleted VM to return 404, got {resp.status_code}: {resp.content}"
        )


# vm_monitor ticks every 30s (qarax/src/vm_monitor.rs).
VM_MONITOR_TICK_SECS = 30


@pytest.mark.asyncio
async def test_created_vm_survives_monitor_tick_and_starts(client):
    """A never-started VM is not defined on its node yet; the monitor must not
    mark it unknown (which made start fail with NotFound)."""
    async with client as c:
        new_vm = NewVm(
            name=f"test-vm-e2e-idle-created-{int(time.time())}",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        vm_id_str = str(await call_api(create_vm, client=c, body=new_vm))
        try:
            await asyncio.sleep(VM_MONITOR_TICK_SECS + 5)
            vm = await call_api(get_vm, client=c, vm_id=vm_id_str)
            assert vm.status == VmStatus.CREATED, f"expected created, got {vm.status}"

            await call_api(start_vm, client=c, vm_id=vm_id_str)
            await wait_for_status(c, vm_id_str, VmStatus.RUNNING)
        finally:
            await call_api(force_stop_vm, client=c, vm_id=vm_id_str)
            await call_api(delete_vm, client=c, vm_id=vm_id_str)


@pytest.mark.asyncio
async def test_vm_exec_runs_command(client):
    """VM exec runs a command inside a guest-agent-enabled VM."""
    async with client as c:
        up_hosts = [
            host for host in await list_hosts.asyncio(client=c) if host.status == HostStatus.UP
        ]
        assert up_hosts, "Expected at least one UP host"
        parked_hosts = up_hosts[1:]
        resources = None
        vm_id_str = None

        try:
            for host in parked_hosts:
                resp = await update_host.asyncio_detailed(
                    client=c,
                    host_id=host.id,
                    body=UpdateHostRequest(status=HostStatus.MAINTENANCE),
                )
                assert resp.status_code == 200, (
                    f"Failed to set host {host.id} to maintenance: "
                    f"HTTP {resp.status_code} {resp.content}"
                )

            template_id, resources = await create_bootable_sandbox_template(c, "cloud_hv")
            new_vm = NewVm(
                name="test-vm-e2e-exec",
                vm_template_id=template_id,
                guest_agent=True,
            )

            vm_id = await call_api(create_vm, client=c, body=new_vm)
            assert vm_id is not None
            vm_id_str = str(vm_id)

            await call_api(start_vm, client=c, vm_id=vm_id_str)
            vm = await wait_for_status(c, vm_id_str, VmStatus.RUNNING)
            assert vm.guest_agent is True

            request = ExecVmRequest(
                command=["/bin/sh", "-c", "printf vm-exec && uname -s"],
                timeout_secs=15,
            )
            deadline = time.time() + VM_OPERATION_TIMEOUT
            response = None
            last_body = ""

            while time.time() < deadline:
                response = await call_api_detailed(
                    exec_vm,
                    client=c,
                    vm_id=vm_id_str,
                    body=request,
                )
                status_code = getattr(response.status_code, "value", response.status_code)
                if status_code == 200:
                    break

                body = getattr(response, "content", b"")
                if isinstance(body, bytes):
                    body = body.decode(errors="replace")
                last_body = body
                assert status_code == 422, (
                    f"Unexpected vm exec status {status_code}: {last_body}"
                )
                await asyncio.sleep(1)
            else:
                raise AssertionError(
                    f"VM exec did not become ready within {VM_OPERATION_TIMEOUT}s: {last_body}"
                )

            result = response.parsed
            assert result is not None
            assert result.exit_code == 0
            assert result.timed_out is False
            assert "vm-exec" in result.stdout
            assert "Linux" in result.stdout
            assert result.stderr == ""
        finally:
            if vm_id_str is not None:
                await call_api(delete_vm, client=c, vm_id=vm_id_str)
            if resources is not None:
                await cleanup_bootable_sandbox_template(c, resources)
            for host in parked_hosts:
                await update_host.asyncio_detailed(
                    client=c,
                    host_id=host.id,
                    body=UpdateHostRequest(status=HostStatus.UP),
                )


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
                await wait_for_status(c, vm_id_str, VmStatus.RUNNING)

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
