"""
E2E tests for live VM migration, host evacuation and placement policies.

These tests verify VM migration between two qarax-node hosts:
- Migrating a running VM with a disk on a shared NFS pool to the other host
  succeeds (job reaches COMPLETED) and leaves the VM RUNNING on the target
- Evacuating a host moves its VMs off and puts it in maintenance
- Reservation classes, anti-affinity and spread tags steer placement

Live migration requires every disk to be on shared storage — OverlayBD and
local disks are not supported. The migration test creates a shared NFS pool,
attaches it to both hosts and attaches a disk from it before starting the VM.

Prerequisites (provided by docker-compose.yml):
- Two qarax-node instances: qarax-node (port 50051) and qarax-node-2 (port 50051)
- A shared NFS server at nfs-server:/nfs-export
"""

import asyncio
import os
import time
import uuid
from uuid import UUID

import httpx
import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import evacuate as evacuate_host
from qarax_api_client.api.hosts import update as update_host
from qarax_api_client.api.jobs import get as get_job
from qarax_api_client.api.storage_objects import delete as delete_storage_object
from qarax_api_client.api.storage_pools import (
    attach_host as attach_pool_host,
)
from qarax_api_client.api.storage_pools import (
    create as create_pool,
)
from qarax_api_client.api.storage_pools import (
    create_disk,
)
from qarax_api_client.api.storage_pools import (
    delete as delete_pool,
)
from qarax_api_client.api.vms import attach_disk, migrate
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.models import (
    AttachDiskRequest,
    AttachPoolHostRequest,
    CreateDiskRequest,
    HostStatus,
    Hypervisor,
    NewStoragePool,
    NewVm,
    StoragePoolType,
    UpdateHostRequest,
)
from qarax_api_client.models.job_status import JobStatus
from qarax_api_client.models.vm_migrate_request import VmMigrateRequest
from qarax_api_client.models.vm_status import VmStatus

from helpers import (
    AUTH_HEADERS,
    call_api,
    call_api_detailed,
    wait_for_host_status,
    wait_for_status,
)

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
NFS_SERVER_HOST = os.getenv("NFS_SERVER_HOST", "nfs-server")
NFS_EXPORT_PATH = os.getenv("NFS_EXPORT_PATH", "/nfs-export")

VM_OPERATION_TIMEOUT = 60
MIGRATION_TIMEOUT = 120


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


async def _wait_for_job(c, job_id, timeout=MIGRATION_TIMEOUT):
    deadline = time.time() + timeout
    while time.time() < deadline:
        job = await call_api(get_job, client=c, job_id=UUID(str(job_id)))
        if job.status == JobStatus.COMPLETED:
            return job
        if job.status == JobStatus.FAILED:
            error = getattr(job, "error", None) or getattr(job, "message", "unknown")
            raise RuntimeError(f"Job {job_id} failed: {error}")
        await asyncio.sleep(1)
    raise TimeoutError(f"Job {job_id} did not complete within {timeout}s")


async def _update_host_placement(
    http_client: httpx.AsyncClient,
    host_id: UUID,
    reservation_class: str | None = None,
    placement_labels: dict[str, str] | None = None,
):
    resp = await http_client.put(
        f"/hosts/{host_id}/placement",
        json={
            "reservation_class": reservation_class,
            "placement_labels": placement_labels or {},
        },
    )
    assert resp.status_code == 200, (
        f"Failed to update placement for host {host_id}: "
        f"HTTP {resp.status_code} {resp.text}"
    )


async def _create_vm_raw(
    http_client: httpx.AsyncClient,
    *,
    name: str,
    tags: list[str] | None = None,
    placement_policy: dict | None = None,
) -> UUID:
    resp = await http_client.post(
        "/vms",
        json={
            "name": name,
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 256 * 1024 * 1024,
            "tags": tags,
            "placement_policy": placement_policy,
        },
    )
    assert resp.status_code == 201, (
        f"Failed to create VM {name}: HTTP {resp.status_code} {resp.text}"
    )
    return UUID(resp.text.strip('"'))


# Full live migration test


@pytest.mark.asyncio
async def test_live_migration(client, two_hosts):
    """
    Full live migration lifecycle:
    - Create NFS pool, attach to both hosts
    - Create a VM and attach a disk from the NFS pool before its first start
    - Start VM, wait for RUNNING
    - Migrate to the other host, wait for job COMPLETED
    - Assert VM is RUNNING and host_id == target host
    """
    host1_id, host2_id = two_hosts
    pool_id = None
    disk_id = None
    vm_id = None

    async with client as c:
        try:
            # Create a shared NFS storage pool
            pool = NewStoragePool(
                name=f"e2e-migrate-nfs-{uuid.uuid4().hex[:6]}",
                pool_type=StoragePoolType.NFS,
                config={"url": f"{NFS_SERVER_HOST}:{NFS_EXPORT_PATH}"},
            )
            pool_id_raw = await call_api(create_pool, client=c, body=pool)
            pool_id = UUID(str(pool_id_raw).strip('"'))

            # Attach NFS pool to both hosts
            for host_id in (host1_id, host2_id):
                await call_api(
                    attach_pool_host,
                    client=c,
                    pool_id=pool_id,
                    body=AttachPoolHostRequest(host_id=host_id),
                )

            # A blank disk on the NFS share: both nodes open the same file.
            disk = await call_api(
                create_disk,
                client=c,
                pool_id=pool_id,
                body=CreateDiskRequest(
                    name=f"e2e-migrate-disk-{uuid.uuid4().hex[:6]}",
                    size_bytes=64 * 1024 * 1024,
                ),
            )
            disk_id = disk.storage_object_id

            # Create the VM
            new_vm = NewVm(
                name="e2e-live-migrate",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=256 * 1024 * 1024,
            )
            result = await call_api(create_vm, client=c, body=new_vm)
            vm_id = UUID(str(result).strip('"'))

            # Attached while Created, the disk is part of the VM config at boot.
            await call_api(
                attach_disk,
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )

            # Start the VM and wait for RUNNING
            await call_api(start_vm, client=c, vm_id=vm_id)
            vm = await wait_for_status(
                c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT
            )
            source_host_id = vm.host_id
            assert source_host_id is not None, "VM should have an assigned host"
            target_host_id = host2_id if source_host_id == host1_id else host1_id

            # Trigger live migration to the other host.
            migrate_resp = await call_api_detailed(
                migrate,
                client=c,
                vm_id=vm_id,
                body=VmMigrateRequest(target_host_id=target_host_id),
            )
            assert migrate_resp.status_code == 202, (
                f"Expected 202 from migrate, got {migrate_resp.status_code}: {migrate_resp.content}"
            )
            job_id = migrate_resp.parsed.job_id

            # Wait for migration job to complete
            await _wait_for_job(c, job_id)

            # Verify VM is RUNNING on the target host
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, (
                f"Expected RUNNING after migration, got: {vm.status}"
            )
            assert vm.host_id == target_host_id, (
                f"Expected host_id={target_host_id} after migration, got: {vm.host_id}"
            )
        finally:
            if vm_id is not None:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if disk_id is not None:
                await call_api(delete_storage_object, client=c, object_id=disk_id)
            if pool_id is not None:
                await call_api(delete_pool, client=c, pool_id=pool_id)


@pytest.mark.asyncio
async def test_host_evacuation_marks_maintenance_and_avoids_rescheduling(client, two_hosts):
    vm_id = None
    extra_vm_id = None
    source_host_id = None

    async with client as c:
        try:
            vm_body = NewVm(
                name=f"e2e-host-evacuate-{uuid.uuid4().hex[:6]}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=256 * 1024 * 1024,
            )
            result = await call_api(create_vm, client=c, body=vm_body)
            vm_id = UUID(str(result).strip('"'))

            await call_api(start_vm, client=c, vm_id=vm_id)
            vm = await wait_for_status(
                c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT
            )
            source_host_id = vm.host_id
            assert source_host_id is not None, "VM should have an assigned host"

            evacuate_resp = await call_api_detailed(
                evacuate_host,
                client=c,
                host_id=source_host_id,
            )
            assert evacuate_resp.status_code == 202, (
                f"Expected 202 from evacuate, got {evacuate_resp.status_code}: {evacuate_resp.content}"
            )
            await _wait_for_job(c, evacuate_resp.parsed.job_id)

            await wait_for_host_status(c, source_host_id, HostStatus.MAINTENANCE)
            vm = await wait_for_status(
                c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT
            )
            assert vm.host_id != source_host_id, "VM should move off the evacuated host"

            extra_vm = NewVm(
                name=f"e2e-post-evacuate-{uuid.uuid4().hex[:6]}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=256 * 1024 * 1024,
            )
            extra_result = await call_api(create_vm, client=c, body=extra_vm)
            extra_vm_id = UUID(str(extra_result).strip('"'))
            extra_vm_state = await call_api(get_vm, client=c, vm_id=extra_vm_id)
            assert (
                extra_vm_state.host_id != source_host_id
            ), "maintenance host should be excluded from new scheduling"
        finally:
            if source_host_id is not None:
                await call_api(
                    update_host,
                    host_id=source_host_id,
                    client=c,
                    body=UpdateHostRequest(status=HostStatus.UP),
                )
            if vm_id is not None:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if extra_vm_id is not None:
                await call_api(delete_vm, client=c, vm_id=extra_vm_id)


@pytest.mark.asyncio
async def test_scheduler_placement_policy_reservation_anti_affinity_and_spread(
    client, two_hosts
):
    host1_id, host2_id = two_hosts
    reservation_class = f"reserved-{uuid.uuid4().hex[:6]}"
    anti_affinity_tag = f"tenant:red-{uuid.uuid4().hex[:6]}"
    spread_tag = f"spread:web-{uuid.uuid4().hex[:6]}"
    reserved_vm_id = None
    anti_vm_id = None
    spread_vm_ids: list[UUID] = []

    async with client as c, httpx.AsyncClient(base_url=QARAX_URL, headers=AUTH_HEADERS) as http_client:
        try:
            await _update_host_placement(
                http_client, host1_id, reservation_class=reservation_class
            )
            await _update_host_placement(http_client, host2_id, reservation_class="general")

            reserved_vm_id = await _create_vm_raw(
                http_client,
                name=f"e2e-placement-reserved-{uuid.uuid4().hex[:6]}",
                tags=[anti_affinity_tag],
                placement_policy={"reservation_class": reservation_class},
            )
            reserved_vm = await get_vm.asyncio(client=c, vm_id=reserved_vm_id)
            assert (
                reserved_vm.host_id == host1_id
            ), "reservation_class should pin placement to the matching host"

            anti_vm_id = await _create_vm_raw(
                http_client,
                name=f"e2e-placement-anti-{uuid.uuid4().hex[:6]}",
                tags=[anti_affinity_tag],
                placement_policy={"anti_affinity_tags": [anti_affinity_tag]},
            )
            anti_vm = await get_vm.asyncio(client=c, vm_id=anti_vm_id)
            assert (
                anti_vm.host_id == host2_id
            ), "anti-affinity should avoid the host that already carries the tag"

            await update_host.asyncio_detailed(
                client=c,
                host_id=host2_id,
                body=UpdateHostRequest(status=HostStatus.MAINTENANCE),
            )
            await wait_for_host_status(c, host2_id, HostStatus.MAINTENANCE)

            for _ in range(2):
                spread_vm_id = await _create_vm_raw(
                    http_client,
                    name=f"e2e-placement-spread-seed-{uuid.uuid4().hex[:6]}",
                    tags=[spread_tag],
                )
                spread_vm_ids.append(spread_vm_id)
                spread_vm = await get_vm.asyncio(client=c, vm_id=spread_vm_id)
                assert (
                    spread_vm.host_id == host1_id
                ), "maintenance host should be excluded from new placement"

            await update_host.asyncio_detailed(
                client=c,
                host_id=host2_id,
                body=UpdateHostRequest(status=HostStatus.UP),
            )
            await wait_for_host_status(c, host2_id, HostStatus.UP)

            spread_vm_id = await _create_vm_raw(
                http_client,
                name=f"e2e-placement-spread-{uuid.uuid4().hex[:6]}",
                tags=[spread_tag],
                placement_policy={"spread_tags": [spread_tag]},
            )
            spread_vm_ids.append(spread_vm_id)
            spread_vm = await get_vm.asyncio(client=c, vm_id=spread_vm_id)
            assert (
                spread_vm.host_id == host2_id
            ), "spread policy should pick the less-populated host once it is schedulable"
        finally:
            await update_host.asyncio_detailed(
                client=c,
                host_id=host1_id,
                body=UpdateHostRequest(status=HostStatus.UP),
            )
            await update_host.asyncio_detailed(
                client=c,
                host_id=host2_id,
                body=UpdateHostRequest(status=HostStatus.UP),
            )
            await _update_host_placement(http_client, host1_id)
            await _update_host_placement(http_client, host2_id)
            for vm_id in spread_vm_ids:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if anti_vm_id is not None:
                await call_api(delete_vm, client=c, vm_id=anti_vm_id)
            if reserved_vm_id is not None:
                await call_api(delete_vm, client=c, vm_id=reserved_vm_id)
