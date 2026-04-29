"""
E2E tests for live VM migration.

These tests verify VM migration between two qarax-node hosts:
- Migrating a running VM to a second host succeeds (job reaches COMPLETED)
- After migration the VM is RUNNING on the target host
- Migrating to the same host is rejected (422)
- Migrating a stopped VM is rejected (422)
- Migrating an unknown VM returns 404

Live migration requires both hosts to share an NFS storage pool — OverlayBD
disks are not supported. The test creates a shared NFS pool, attaches it to
both hosts, and creates a VM disk on that pool before starting the VM.

Prerequisites (provided by docker-compose.yml):
- Two qarax-node instances: qarax-node (port 50051) and qarax-node-2 (port 50051)
- A shared NFS server at nfs-server:/nfs-export
"""

import asyncio
import os
import time
import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import add as add_host
from qarax_api_client.api.hosts import evacuate as evacuate_host
from qarax_api_client.api.hosts import init as init_host
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.hosts import update as update_host
from qarax_api_client.api.jobs import get as get_job
from qarax_api_client.api.storage_objects import (
    create as create_storage_object,
    delete as delete_storage_object,
)
from qarax_api_client.api.storage_pools import (
    attach_host as attach_pool_host,
    create as create_pool,
    delete as delete_pool,
)
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.api.vms import migrate
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.api.vms import stop as stop_vm
from qarax_api_client.models import (
    HostStatus,
    Hypervisor,
    NewStoragePool,
    NewVm,
    StoragePoolType,
    UpdateHostRequest,
)
from qarax_api_client.models.attach_host_request import AttachHostRequest
from qarax_api_client.models.job_status import JobStatus
from qarax_api_client.models.new_host import NewHost
from qarax_api_client.models.new_storage_object import NewStorageObject
from qarax_api_client.models.storage_object_type import StorageObjectType
from qarax_api_client.models.vm_migrate_request import VmMigrateRequest
from qarax_api_client.models.vm_status import VmStatus

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
QARAX_NODE_HOST = os.getenv("QARAX_NODE_HOST", "qarax-node")
QARAX_NODE_PORT = int(os.getenv("QARAX_NODE_PORT", "50051"))
QARAX_NODE_2_HOST = os.getenv("QARAX_NODE_2_HOST", "qarax-node-2")
QARAX_NODE_2_PORT = int(os.getenv("QARAX_NODE_2_PORT", "50051"))
NFS_SERVER_HOST = os.getenv("NFS_SERVER_HOST", "nfs-server")
NFS_EXPORT_PATH = os.getenv("NFS_EXPORT_PATH", "/nfs-export")

VM_OPERATION_TIMEOUT = 60
MIGRATION_TIMEOUT = 120


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


async def _wait_for_job(c, job_id, timeout=MIGRATION_TIMEOUT):
    start = time.time()
    while time.time() - start < timeout:
        job = await get_job.asyncio(client=c, job_id=UUID(str(job_id)))
        if job is None:
            raise RuntimeError(f"Job {job_id} not found")
        if job.status == JobStatus.COMPLETED:
            return job
        if job.status == JobStatus.FAILED:
            error = getattr(job, "error", None) or getattr(job, "message", "unknown")
            raise RuntimeError(f"Migration job {job_id} failed: {error}")
        await asyncio.sleep(1)
    raise TimeoutError(f"Migration job {job_id} did not complete within {timeout}s")


async def _wait_for_host_status(c, host_id, expected_status, timeout=MIGRATION_TIMEOUT):
    start = time.time()
    while time.time() - start < timeout:
        hosts = await list_hosts.asyncio(client=c)
        host = next((h for h in hosts or [] if h.id == host_id), None)
        if host is not None and host.status == expected_status:
            return host
        await asyncio.sleep(1)
    hosts = await list_hosts.asyncio(client=c)
    host = next((h for h in hosts or [] if h.id == host_id), None)
    current = host.status if host is not None else "missing"
    raise TimeoutError(
        f"Host {host_id} did not reach {expected_status} within {timeout}s. Current: {current}"
    )


# Negative tests (no VM boot required)────────


@pytest.mark.asyncio
async def test_migrate_unknown_vm_returns_404(client, two_hosts):
    """Migrating an unknown VM ID returns 404."""
    _, host2_id = two_hosts
    async with client as c:
        resp = await migrate.asyncio_detailed(
            client=c,
            vm_id=uuid.uuid4(),
            body=VmMigrateRequest(target_host_id=host2_id),
        )
        assert resp.status_code == 404, f"Expected 404, got {resp.status_code}"


@pytest.mark.asyncio
async def test_migrate_stopped_vm_returns_422(client, two_hosts):
    """Migrating a VM that has never been started (status=created) returns 422."""
    _, host2_id = two_hosts
    async with client as c:
        new_vm = NewVm(
            name="e2e-migrate-stopped",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        result = await create_vm.asyncio(client=c, body=new_vm)
        vm_id = UUID(str(result).strip('"'))

        try:
            resp = await migrate.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmMigrateRequest(target_host_id=host2_id),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for stopped VM, got {resp.status_code}"
            )
        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_migrate_to_same_host_returns_422(client, two_hosts):
    """Migrating a VM to its current host returns 422."""
    async with client as c:
        new_vm = NewVm(
            name="e2e-migrate-same-host",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        result = await create_vm.asyncio(client=c, body=new_vm)
        vm_id = UUID(str(result).strip('"'))

        try:
            # Start the VM so it is in RUNNING state (required for migration validation to reach
            # the same-host check), then attempt to migrate to the same host.
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            vm = await _wait_for_vm_status(c, vm_id, VmStatus.RUNNING)
            current_host_id = vm.host_id
            assert current_host_id is not None, "VM should have an assigned host"

            resp = await migrate.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=VmMigrateRequest(target_host_id=current_host_id),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for same-host migration, got {resp.status_code}"
            )

            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


# Full live migration test────────


@pytest.mark.asyncio
async def test_live_migration(client, two_hosts):
    """
    Full live migration lifecycle:
    - Create NFS pool, attach to both hosts
    - Create VM with NFS-backed disk
    - Start VM, wait for RUNNING
    - Migrate to host2, wait for job COMPLETED
    - Assert VM is RUNNING and host_id == host2
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
            pool_id_raw = await create_pool.asyncio(client=c, body=pool)
            pool_id = UUID(str(pool_id_raw).strip('"'))

            # Attach NFS pool to both hosts
            for host_id in (host1_id, host2_id):
                resp = await attach_pool_host.asyncio_detailed(
                    client=c,
                    pool_id=pool_id,
                    body=AttachHostRequest(host_id=host_id, bridge_name="unused"),
                )
                assert resp.status_code in (200, 201, 204), (
                    f"Failed to attach NFS pool to host {host_id}: HTTP {resp.status_code}"
                )

            # Create a disk on the NFS pool
            obj = NewStorageObject(
                name=f"e2e-migrate-disk-{uuid.uuid4().hex[:6]}",
                storage_pool_id=str(pool_id),
                object_type=StorageObjectType.DISK,
                size_bytes=512 * 1024 * 1024,
            )
            disk_id_raw = await create_storage_object.asyncio(client=c, body=obj)
            disk_id = UUID(str(disk_id_raw).strip('"'))

            # Create the VM
            new_vm = NewVm(
                name="e2e-live-migrate",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=256 * 1024 * 1024,
            )
            result = await create_vm.asyncio(client=c, body=new_vm)
            vm_id = UUID(str(result).strip('"'))

            # Start the VM and wait for RUNNING
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            vm = await _wait_for_vm_status(c, vm_id, VmStatus.RUNNING)
            source_host_id = vm.host_id
            assert source_host_id is not None, "VM should have an assigned host"
            target_host_id = host2_id if source_host_id == host1_id else host1_id

            # Trigger live migration to the other host.
            migrate_resp = await migrate.asyncio_detailed(
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
            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, (
                f"Expected RUNNING after migration, got: {vm.status}"
            )
            assert vm.host_id == target_host_id, (
                f"Expected host_id={target_host_id} after migration, got: {vm.host_id}"
            )

            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)

        finally:
            if vm_id is not None:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if disk_id is not None:
                try:
                    await delete_storage_object.asyncio_detailed(
                        client=c, object_id=disk_id
                    )
                except Exception:
                    pass
            if pool_id is not None:
                try:
                    await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)
                except Exception:
                    pass


@pytest.mark.asyncio
async def test_host_evacuation_marks_maintenance_and_avoids_rescheduling(client, two_hosts):
    host1_id, host2_id = two_hosts
    pool_id = None
    disk_id = None
    vm_id = None
    extra_vm_id = None
    source_host_id = None

    async with client as c:
        try:
            pool = NewStoragePool(
                name=f"e2e-evacuate-nfs-{uuid.uuid4().hex[:6]}",
                pool_type=StoragePoolType.NFS,
                config={"url": f"{NFS_SERVER_HOST}:{NFS_EXPORT_PATH}"},
            )
            pool_id_raw = await create_pool.asyncio(client=c, body=pool)
            pool_id = UUID(str(pool_id_raw).strip('"'))

            for host_id in (host1_id, host2_id):
                resp = await attach_pool_host.asyncio_detailed(
                    client=c,
                    pool_id=pool_id,
                    body=AttachHostRequest(host_id=host_id, bridge_name="unused"),
                )
                assert resp.status_code in (200, 201, 204), (
                    f"Failed to attach NFS pool to host {host_id}: HTTP {resp.status_code}"
                )

            obj = NewStorageObject(
                name=f"e2e-evacuate-disk-{uuid.uuid4().hex[:6]}",
                storage_pool_id=str(pool_id),
                object_type=StorageObjectType.DISK,
                size_bytes=512 * 1024 * 1024,
            )
            disk_id_raw = await create_storage_object.asyncio(client=c, body=obj)
            disk_id = UUID(str(disk_id_raw).strip('"'))

            vm_body = NewVm(
                name=f"e2e-host-evacuate-{uuid.uuid4().hex[:6]}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=256 * 1024 * 1024,
            )
            result = await create_vm.asyncio(client=c, body=vm_body)
            vm_id = UUID(str(result).strip('"'))

            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            vm = await _wait_for_vm_status(c, vm_id, VmStatus.RUNNING)
            source_host_id = vm.host_id
            assert source_host_id is not None, "VM should have an assigned host"

            evacuate_resp = await evacuate_host.asyncio_detailed(
                client=c,
                host_id=source_host_id,
            )
            assert evacuate_resp.status_code == 202, (
                f"Expected 202 from evacuate, got {evacuate_resp.status_code}: {evacuate_resp.content}"
            )
            await _wait_for_job(c, evacuate_resp.parsed.job_id)

            await _wait_for_host_status(c, source_host_id, HostStatus.MAINTENANCE)
            vm = await _wait_for_vm_status(c, vm_id, VmStatus.RUNNING)
            assert vm.host_id != source_host_id, "VM should move off the evacuated host"

            extra_vm = NewVm(
                name=f"e2e-post-evacuate-{uuid.uuid4().hex[:6]}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=256 * 1024 * 1024,
            )
            extra_result = await create_vm.asyncio(client=c, body=extra_vm)
            extra_vm_id = UUID(str(extra_result).strip('"'))
            extra_vm_state = await get_vm.asyncio(client=c, vm_id=extra_vm_id)
            assert (
                extra_vm_state.host_id != source_host_id
            ), "maintenance host should be excluded from new scheduling"
        finally:
            if source_host_id is not None:
                await update_host.asyncio_detailed(
                    host_id=source_host_id,
                    client=c,
                    body=UpdateHostRequest(status=HostStatus.UP),
                )
            if vm_id is not None:
                try:
                    await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
                except Exception:
                    pass
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if extra_vm_id is not None:
                await delete_vm.asyncio_detailed(client=c, vm_id=extra_vm_id)
            if disk_id is not None:
                try:
                    await delete_storage_object.asyncio_detailed(
                        client=c, object_id=disk_id
                    )
                except Exception:
                    pass
            if pool_id is not None:
                try:
                    await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)
                except Exception:
                    pass
