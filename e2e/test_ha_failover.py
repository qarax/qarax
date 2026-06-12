"""
E2E tests for automatic HA failover.

These tests verify that when a host dies, VMs created with ha_enabled are
automatically restarted on another host, while non-HA VMs stay put:
- Both VMs are pinned to one host (the other is put in maintenance during
  creation), then that host's qarax-node container is stopped.
- The control plane marks the host DOWN, and after the HA grace period
  (HA_FAILOVER_GRACE_SECONDS, set low in docker-compose.yml) the HA monitor
  cold-restarts the HA VM on the surviving host.
- The non-HA VM must not be rescheduled.

Prerequisites (provided by docker-compose.yml):
- Two qarax-node instances: qarax-node (host1) and qarax-node-2 (host2)
- HA_FAILOVER_GRACE_SECONDS / HA_CHECK_INTERVAL_SECONDS set to a few seconds

The test manipulates the docker compose stack (stop/start of a node
service), so it must run against the standard e2e compose project.
"""

import asyncio
import os
import subprocess
import time
import uuid
from pathlib import Path
from uuid import UUID

import httpx
import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import add as add_host
from qarax_api_client.api.hosts import init as init_host
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.hosts import update as update_host
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.api.vms import stop as stop_vm
from qarax_api_client.models import HostStatus, UpdateHostRequest
from qarax_api_client.models.new_host import NewHost
from qarax_api_client.models.vm_status import VmStatus

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
QARAX_NODE_HOST = os.getenv("QARAX_NODE_HOST", "qarax-node")
QARAX_NODE_PORT = int(os.getenv("QARAX_NODE_PORT", "50051"))
QARAX_NODE_2_HOST = os.getenv("QARAX_NODE_2_HOST", "qarax-node-2")
QARAX_NODE_2_PORT = int(os.getenv("QARAX_NODE_2_PORT", "50051"))

E2E_DIR = Path(__file__).parent

VM_OPERATION_TIMEOUT = 60
# Host-down detection (resource monitor tick, 30s) + HA grace + HA tick + boot
FAILOVER_TIMEOUT = 180


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


def _register_and_init_host(client, address, port, name):
    """Register a host if not already present and initialize it. Returns host_id."""
    hosts = list_hosts.sync(client=client)
    if hosts is None:
        raise RuntimeError("Failed to list hosts")

    existing = next((h for h in hosts if h.address == address), None)
    if existing is not None:
        host_id = existing.id
    else:
        new_host = NewHost(
            name=name, address=address, port=port, host_user="root", password=""
        )
        result = add_host.sync_detailed(client=client, body=new_host)
        if result.status_code.value == 201:
            host_id = UUID(result.parsed.strip())
        else:
            hosts = list_hosts.sync(client=client)
            existing = next((h for h in hosts if h.address == address), None)
            if existing is None:
                raise RuntimeError(f"Could not register host at {address}:{port}")
            host_id = existing.id

    result = init_host.sync_detailed(host_id=host_id, client=client)
    if result.status_code.value != 200:
        raise RuntimeError(
            f"Failed to initialize host {host_id}: HTTP {result.status_code}"
        )
    return host_id


@pytest.fixture(scope="module")
def two_hosts():
    """Ensure both qarax-node instances are registered and initialized. Returns (host1_id, host2_id)."""
    c = Client(base_url=QARAX_URL)
    host1_id = _register_and_init_host(
        c, QARAX_NODE_HOST, QARAX_NODE_PORT, "e2e-node-1"
    )
    host2_id = _register_and_init_host(
        c, QARAX_NODE_2_HOST, QARAX_NODE_2_PORT, "e2e-node-2"
    )
    return host1_id, host2_id


def _compose(*args):
    subprocess.run(
        ["docker", "compose", *args],
        cwd=E2E_DIR,
        check=True,
        capture_output=True,
    )


async def _wait_for_vm_status(c, vm_id, expected_status, timeout=VM_OPERATION_TIMEOUT):
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


async def _wait_for_host_status(c, host_id, expected_status, timeout=120):
    start = time.time()
    while time.time() - start < timeout:
        hosts = await list_hosts.asyncio(client=c)
        host = next((h for h in hosts or [] if h.id == host_id), None)
        if host is not None and host.status == expected_status:
            return host
        await asyncio.sleep(1)
    raise TimeoutError(f"Host {host_id} did not reach {expected_status} within {timeout}s")


async def _wait_for_failover(c, vm_id, source_host_id, timeout=FAILOVER_TIMEOUT):
    """Wait until the VM is RUNNING on a host other than source_host_id."""
    start = time.time()
    while time.time() - start < timeout:
        vm = await get_vm.asyncio(client=c, vm_id=vm_id)
        if vm.status == VmStatus.RUNNING and vm.host_id != source_host_id:
            return vm
        await asyncio.sleep(2)
    vm = await get_vm.asyncio(client=c, vm_id=vm_id)
    raise TimeoutError(
        f"VM {vm_id} did not fail over within {timeout}s. "
        f"Current: status={vm.status} host_id={vm.host_id}"
    )


async def _create_vm_raw(http_client, *, name, ha_enabled):
    resp = await http_client.post(
        "/vms",
        json={
            "name": name,
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 256 * 1024 * 1024,
            "ha_enabled": ha_enabled,
        },
    )
    assert resp.status_code == 201, (
        f"Failed to create VM {name}: HTTP {resp.status_code} {resp.text}"
    )
    return UUID(resp.text.strip('"'))


@pytest.mark.asyncio
async def test_ha_enabled_is_persisted(client, two_hosts):
    """ha_enabled survives the round trip through create and get."""
    async with httpx.AsyncClient(base_url=QARAX_URL) as http_client:
        vm_id = await _create_vm_raw(
            http_client, name=f"e2e-ha-flag-{uuid.uuid4().hex[:6]}", ha_enabled=True
        )
        try:
            resp = await http_client.get(f"/vms/{vm_id}")
            assert resp.status_code == 200
            assert resp.json()["ha_enabled"] is True
        finally:
            await http_client.delete(f"/vms/{vm_id}")


@pytest.mark.asyncio
async def test_ha_failover_on_host_failure(client, two_hosts):
    """
    Full HA failover lifecycle:
    - Pin one HA VM and one non-HA VM to host A (host B in maintenance)
    - Stop host A's qarax-node container
    - Assert the HA VM is restarted RUNNING on host B
    - Assert the non-HA VM is not rescheduled
    """
    host1_id, host2_id = two_hosts
    node_service_by_host = {host1_id: "qarax-node", host2_id: "qarax-node-2"}

    ha_vm_id = None
    plain_vm_id = None
    dead_host_id = None
    stopped_service = None

    async with client as c, httpx.AsyncClient(base_url=QARAX_URL) as http_client:
        try:
            # Pin both VMs to host1 by putting host2 in maintenance.
            await update_host.asyncio_detailed(
                client=c,
                host_id=host2_id,
                body=UpdateHostRequest(status=HostStatus.MAINTENANCE),
            )
            await _wait_for_host_status(c, host2_id, HostStatus.MAINTENANCE)

            ha_vm_id = await _create_vm_raw(
                http_client,
                name=f"e2e-ha-failover-{uuid.uuid4().hex[:6]}",
                ha_enabled=True,
            )
            plain_vm_id = await _create_vm_raw(
                http_client,
                name=f"e2e-ha-plain-{uuid.uuid4().hex[:6]}",
                ha_enabled=False,
            )

            for vm_id in (ha_vm_id, plain_vm_id):
                await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
                vm = await _wait_for_vm_status(c, vm_id, VmStatus.RUNNING)
                assert vm.host_id == host1_id, (
                    f"VM {vm_id} should be pinned to host1, got {vm.host_id}"
                )

            # Make host2 schedulable again so failover has a destination.
            await update_host.asyncio_detailed(
                client=c,
                host_id=host2_id,
                body=UpdateHostRequest(status=HostStatus.UP),
            )
            await _wait_for_host_status(c, host2_id, HostStatus.UP)

            # Kill host1's node.
            dead_host_id = host1_id
            stopped_service = node_service_by_host[host1_id]
            _compose("stop", stopped_service)

            # The HA VM must come back RUNNING on the surviving host.
            vm = await _wait_for_failover(c, ha_vm_id, dead_host_id)
            assert vm.host_id == host2_id, (
                f"Expected HA VM on host2 after failover, got {vm.host_id}"
            )

            # The dead host must be marked DOWN.
            await _wait_for_host_status(c, dead_host_id, HostStatus.DOWN, timeout=30)

            # The non-HA VM must not have been rescheduled.
            plain_vm = await get_vm.asyncio(client=c, vm_id=plain_vm_id)
            assert plain_vm.host_id == dead_host_id, (
                "non-HA VM should not be failed over"
            )

            await stop_vm.asyncio_detailed(client=c, vm_id=ha_vm_id)
        finally:
            if stopped_service is not None:
                _compose("start", stopped_service)
                # Give the recovered node time to become reachable so the HA
                # monitor can clean up stale copies and deletes can route.
                await asyncio.sleep(10)
            if dead_host_id is not None:
                await update_host.asyncio_detailed(
                    host_id=dead_host_id,
                    client=c,
                    body=UpdateHostRequest(status=HostStatus.UP),
                )
                await _wait_for_host_status(c, dead_host_id, HostStatus.UP)
            for vm_id in (ha_vm_id, plain_vm_id):
                if vm_id is None:
                    continue
                try:
                    await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
                except Exception:
                    pass
                try:
                    await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
                except Exception:
                    pass
