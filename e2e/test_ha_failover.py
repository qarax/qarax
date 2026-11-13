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
from qarax_api_client.api.hosts import init as init_host
from qarax_api_client.api.hosts import update as update_host
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.models import HostStatus, UpdateHostRequest
from qarax_api_client.models.vm_status import VmStatus

from helpers import (
    AUTH_HEADERS,
    DOCKER_TIMEOUT,
    call_api,
    call_api_detailed,
    wait_for_host_status,
    wait_for_status,
)

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")

E2E_DIR = Path(__file__).parent

VM_OPERATION_TIMEOUT = 60
# Host-down detection (resource monitor tick, 30s) + HA grace + HA tick + boot
FAILOVER_TIMEOUT = 180
# The resource monitor marks an unreachable host DOWN on its next 30s tick, so
# waiting exactly one period can miss it.
HOST_DOWN_TIMEOUT = 60
# How long a restarted qarax-node may take to accept gRPC again.
NODE_RESTART_TIMEOUT = 120


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


def _compose(*args):
    subprocess.run(
        ["docker", "compose", *args],
        cwd=E2E_DIR,
        check=True,
        capture_output=True,
        timeout=DOCKER_TIMEOUT,
    )


async def _reinit_host(c, host_id, timeout=NODE_RESTART_TIMEOUT):
    """Initialize a host whose node was restarted, retrying until its gRPC port is back.

    Init probes the node and only then marks the host UP; a status PATCH would
    only write the database.
    """
    deadline = time.time() + timeout
    while True:
        resp = await call_api_detailed(init_host, client=c, host_id=host_id)
        if resp.status_code == 200:
            return
        if time.time() >= deadline:
            raise TimeoutError(
                f"init of host {host_id} still failing after {timeout}s: "
                f"HTTP {resp.status_code.value} {resp.content.decode(errors='replace')}"
            )
        await asyncio.sleep(2)


async def _wait_for_failover(c, vm_id, source_host_id, timeout=FAILOVER_TIMEOUT):
    """Wait until the VM is RUNNING on a host other than source_host_id."""
    deadline = time.time() + timeout
    while True:
        vm = await call_api(get_vm, client=c, vm_id=vm_id)
        if vm.status == VmStatus.RUNNING and vm.host_id != source_host_id:
            return vm
        if time.time() >= deadline:
            raise TimeoutError(
                f"VM {vm_id} did not fail over within {timeout}s. "
                f"Current: status={vm.status} host_id={vm.host_id}"
            )
        await asyncio.sleep(2)


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
    host2_in_maintenance = False
    dead_host_id = None
    stopped_service = None

    async with client as c, httpx.AsyncClient(base_url=QARAX_URL, headers=AUTH_HEADERS) as http_client:
        try:
            # Pin both VMs to host1 by putting host2 in maintenance.
            await call_api(
                update_host,
                client=c,
                host_id=host2_id,
                body=UpdateHostRequest(status=HostStatus.MAINTENANCE),
            )
            host2_in_maintenance = True
            await wait_for_host_status(c, host2_id, HostStatus.MAINTENANCE)

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
                await call_api(start_vm, client=c, vm_id=vm_id)
                vm = await wait_for_status(
                    c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT
                )
                assert vm.host_id == host1_id, (
                    f"VM {vm_id} should be pinned to host1, got {vm.host_id}"
                )

            # Make host2 schedulable again so failover has a destination.
            await call_api(
                update_host,
                client=c,
                host_id=host2_id,
                body=UpdateHostRequest(status=HostStatus.UP),
            )
            host2_in_maintenance = False
            await wait_for_host_status(c, host2_id, HostStatus.UP)

            # Kill host1's node.
            dead_host_id = host1_id
            stopped_service = node_service_by_host[host1_id]
            _compose("stop", stopped_service)

            # The dead host must be marked DOWN before failover can start.
            await wait_for_host_status(
                c, dead_host_id, HostStatus.DOWN, timeout=HOST_DOWN_TIMEOUT
            )

            # The HA VM must come back RUNNING on the surviving host.
            vm = await _wait_for_failover(c, ha_vm_id, dead_host_id)
            assert vm.host_id == host2_id, (
                f"Expected HA VM on host2 after failover, got {vm.host_id}"
            )

            # The non-HA VM must not have been rescheduled.
            plain_vm = await call_api(get_vm, client=c, vm_id=plain_vm_id)
            assert plain_vm.host_id == dead_host_id, (
                "non-HA VM should not be failed over"
            )
        finally:
            if host2_in_maintenance:
                await call_api(
                    update_host,
                    client=c,
                    host_id=host2_id,
                    body=UpdateHostRequest(status=HostStatus.UP),
                )
            if stopped_service is not None:
                _compose("start", stopped_service)
                await _reinit_host(c, dead_host_id)
            # Delete tolerates node errors, so it also cleans up the VM left
            # on the restarted node.
            for vm_id in (ha_vm_id, plain_vm_id):
                if vm_id is not None:
                    await call_api(delete_vm, client=c, vm_id=vm_id)
