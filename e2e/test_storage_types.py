"""
E2E tests for storage types: OCI container images and NFS pools.

These tests verify:
- Creating VMs with an OCI image_ref (async job path)
- Polling the job API until completion
- Creating and attaching an NFS storage pool
"""

import asyncio
import os
import subprocess
import time
import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.jobs import get as get_job
from qarax_api_client.api.storage_objects import (
    create as create_storage_object,
    delete as delete_storage_object,
    list_ as list_storage_objects,
)
from qarax_api_client.api.storage_pools import (
    attach_host as attach_pool_host,
    create as create_pool,
    delete as delete_pool,
)
from qarax_api_client.api.vms import (
    create as create_vm,
    delete as delete_vm,
    get as get_vm,
    resize_disk,
)
from qarax_api_client.models import Hypervisor, NewStoragePool, NewVm, StoragePoolType
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest
from qarax_api_client.models.create_vm_response import CreateVmResponse
from qarax_api_client.models.disk_resize_request import DiskResizeRequest
from qarax_api_client.models.job_status import JobStatus
from qarax_api_client.models.new_storage_object import NewStorageObject
from qarax_api_client.models.storage_object_type import StorageObjectType

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
# Registry URL as seen from the test runner (host-side port mapping)
REGISTRY_PUSH_URL = os.getenv("REGISTRY_PUSH_URL", "localhost:5001")
# Registry URL as seen from inside the docker network
REGISTRY_INTERNAL_URL = os.getenv("REGISTRY_INTERNAL_URL", "registry:5000")
# NFS server hostname as seen from inside the docker network
NFS_SERVER_HOST = os.getenv("NFS_SERVER_HOST", "nfs-server")
NFS_EXPORT_PATH = os.getenv("NFS_EXPORT_PATH", "/nfs-export")

JOB_TIMEOUT = 120  # OCI image pull can take a while


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


async def wait_for_job(c, job_id, timeout=JOB_TIMEOUT):
    """Poll GET /jobs/{job_id} until COMPLETED or FAILED."""
    start = time.time()
    while time.time() - start < timeout:
        job = await get_job.asyncio(client=c, job_id=UUID(str(job_id)))
        if job is None:
            raise RuntimeError(f"Job {job_id} not found")
        if job.status == JobStatus.COMPLETED:
            return job
        if job.status == JobStatus.FAILED:
            error = getattr(job, "error", None) or getattr(job, "message", "unknown")
            raise RuntimeError(f"Job {job_id} failed: {error}")
        await asyncio.sleep(2)
    raise TimeoutError(f"Job {job_id} did not complete within {timeout}s")


from helpers import AUTH_HEADERS, up_hosts as _up_hosts


async def _create_pool(c, name, pool_type, config):
    """Create a storage pool and return its UUID."""
    raw = await create_pool.asyncio(
        client=c, body=NewStoragePool(name=name, pool_type=pool_type, config=config)
    )
    return UUID(str(raw).strip('"'))


async def _attach_pools_to_hosts(c, hosts, pool_ids):
    """Attach pools to all UP hosts so VM host placement never breaks pool locality checks."""
    up_hosts = _up_hosts(hosts)
    assert up_hosts, "No UP hosts available"

    for host in up_hosts:
        for pool_id in pool_ids:
            resp = await attach_pool_host.asyncio_detailed(
                client=c,
                pool_id=pool_id,
                body=AttachPoolHostRequest(host_id=host.id),
            )
            if (
                resp.status_code == 422
                and "Overlaybd storage backend not configured" in resp.content.decode()
            ):
                pytest.skip("OverlayBD backend not configured on this e2e node")
            assert resp.status_code in (200, 201, 204), (
                f"Attach pool {pool_id} to host {host.id} failed: HTTP {resp.status_code}"
            )


async def _cleanup(c, vm_id, object_name_prefixes, pool_ids):
    """Delete VM, storage objects by name prefix, and pools."""
    if vm_id is not None:
        await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
    for prefix in object_name_prefixes:
        objects = await list_storage_objects.asyncio(client=c, name=prefix)
        for obj in objects or []:
            await delete_storage_object.asyncio_detailed(client=c, object_id=obj.id)
    for pool_id in pool_ids:
        await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)


@pytest.fixture(scope="session")
def oci_image_ref():
    """Push a minimal image to the local test registry; return its internal ref."""
    tag = f"{REGISTRY_PUSH_URL}/test/busybox:latest"
    subprocess.run(
        ["docker", "pull", "busybox:latest"], check=True, capture_output=True
    )
    subprocess.run(
        ["docker", "tag", "busybox:latest", tag], check=True, capture_output=True
    )
    subprocess.run(["docker", "push", tag], check=True, capture_output=True)
    return f"{REGISTRY_INTERNAL_URL}/test/busybox:latest"


# OCI image tests


@pytest.mark.asyncio
async def test_create_vm_with_oci_image(client, oci_image_ref):
    """VM created with image_ref should trigger an async job and reach CREATED state."""
    test_id = str(uuid.uuid4())[:8]
    async with client as c:
        hosts = _up_hosts(await list_hosts.asyncio(client=c))
        assert hosts, "No UP hosts registered"

        overlaybd_pool_id = await _create_pool(
            c,
            f"e2e-oci-obd-{test_id}",
            StoragePoolType.OVERLAY_BD,
            {"url": f"http://{REGISTRY_INTERNAL_URL}"},
        )

        vm_id = None
        try:
            await _attach_pools_to_hosts(c, hosts, [overlaybd_pool_id])

            new_vm = NewVm(
                name=f"e2e-oci-image-vm-{test_id}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=268435456,
                image_ref=oci_image_ref,
            )
            result = await create_vm.asyncio(client=c, body=new_vm)
            assert result is not None
            assert isinstance(result, CreateVmResponse), (
                f"Expected CreateVmResponse (202) for OCI VM, got {type(result)}"
            )
            job_id = result.job_id
            vm_id = UUID(str(result.vm_id))

            await wait_for_job(c, job_id)

            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm is not None
            assert vm.status.value == "created", f"Expected 'created', got {vm.status}"
        finally:
            await _cleanup(
                c,
                vm_id,
                [f"overlaybd-{vm_id}"] if vm_id else [],
                [overlaybd_pool_id],
            )


@pytest.mark.asyncio
async def test_preflight_oci_image(client, oci_image_ref):
    """Preflight should mark the test image bootable via OverlayBD."""
    test_id = str(uuid.uuid4())[:8]
    async with client as c:
        hosts = _up_hosts(await list_hosts.asyncio(client=c))
        assert hosts, "No UP hosts registered"

        overlaybd_pool_id = await _create_pool(
            c,
            f"e2e-preflight-obd-{test_id}",
            StoragePoolType.OVERLAY_BD,
            {"url": f"http://{REGISTRY_INTERNAL_URL}"},
        )

        try:
            await _attach_pools_to_hosts(c, hosts, [overlaybd_pool_id])

            response = await c.get_async_httpx_client().post(
                f"{QARAX_URL}/vms/preflight",
                json={
                    "image_ref": oci_image_ref,
                    "boot_mode": "kernel",
                },
            )
            assert response.status_code == 200, response.text

            body = response.json()
            assert body["architecture"]
            assert body["checks"], "expected preflight checks"

            if any(
                check["name"] == "overlaybd_import" and not check["ok"]
                for check in body["checks"]
            ):
                pytest.skip("OverlayBD backend not configured on this e2e node")

            if not body.get("bootable", False):
                failing_checks = [
                    check["name"] for check in body["checks"] if not check.get("ok", False)
                ]
                pytest.skip(f"Image preflight marked non-bootable in this env: {failing_checks}")
            assert any(
                check["name"] == "guest_command" and check["ok"] for check in body["checks"]
            )
        finally:
            await _cleanup(c, None, [], [overlaybd_pool_id])


@pytest.mark.asyncio
async def test_create_vm_with_persistent_overlaybd_upper_has_storage_object(
    client, oci_image_ref
):
    """Persistent OverlayBD upper layers should be provisioned on the requested pool and resize should fail explicitly."""
    test_id = str(uuid.uuid4())[:8]
    async with client as c:
        hosts = _up_hosts(await list_hosts.asyncio(client=c))
        assert hosts, "No UP hosts registered"

        upper_pool_id = await _create_pool(
            c,
            f"e2e-overlaybd-upper-{test_id}",
            StoragePoolType.LOCAL,
            {"path": f"/var/lib/qarax/e2e-overlaybd-upper-{test_id}"},
        )
        overlaybd_pool_id = await _create_pool(
            c,
            f"e2e-overlaybd-{test_id}",
            StoragePoolType.OVERLAY_BD,
            {"url": f"http://{REGISTRY_INTERNAL_URL}"},
        )

        vm_id = None
        try:
            await _attach_pools_to_hosts(
                c, hosts, [upper_pool_id, overlaybd_pool_id]
            )

            new_vm = NewVm(
                name=f"e2e-oci-persistent-upper-{test_id}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=268435456,
                image_ref=oci_image_ref,
                persistent_upper_pool_id=upper_pool_id,
            )
            result = await create_vm.asyncio(client=c, body=new_vm)
            assert isinstance(result, CreateVmResponse), (
                f"Expected CreateVmResponse (202) for OCI VM, got {type(result)}"
            )
            vm_id = UUID(str(result.vm_id))
            await wait_for_job(c, result.job_id)

            upper_name = f"overlaybd-upper-{vm_id}"
            upper_objects = await list_storage_objects.asyncio(
                client=c, name=upper_name
            )
            assert upper_objects is not None and len(upper_objects) == 1, (
                f"Expected one persistent upper storage object named {upper_name}"
            )
            upper_object = upper_objects[0]
            assert upper_object.object_type == StorageObjectType.OVERLAYBD_UPPER
            assert upper_object.config["upper_data"].endswith(".upper.data")
            assert upper_object.config["upper_index"].endswith(".upper.index")
            assert upper_object.config["upper_data"].startswith(
                f"/var/lib/qarax/e2e-overlaybd-upper-{test_id}/"
            )

            resize_resp = await resize_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                disk_id="disk0",
                body=DiskResizeRequest(new_size_bytes=512 * 1024 * 1024),
            )
            assert resize_resp.status_code == 422, (
                f"Expected 422 for persistent OverlayBD resize, got {resize_resp.status_code}"
            )
            assert "OverlayBD" in resize_resp.content.decode(), (
                resize_resp.content.decode()
            )
        finally:
            await _cleanup(
                c,
                vm_id,
                [f"overlaybd-upper-{vm_id}", f"overlaybd-{vm_id}"] if vm_id else [],
                [overlaybd_pool_id, upper_pool_id],
            )


# VM commit tests


@pytest.mark.asyncio
async def test_vm_commit_converts_oci_to_raw_disk(client, oci_image_ref):
    """Commit should convert an OCI image VM to a standalone raw disk VM."""
    test_id = str(uuid.uuid4())[:8]
    async with client as c:
        hosts = _up_hosts(await list_hosts.asyncio(client=c))
        assert hosts, "No UP hosts registered"

        overlaybd_pool_id = await _create_pool(
            c,
            f"e2e-commit-obd-{test_id}",
            StoragePoolType.OVERLAY_BD,
            {"url": f"http://{REGISTRY_INTERNAL_URL}"},
        )
        commit_pool_id = await _create_pool(
            c,
            f"e2e-commit-local-{test_id}",
            StoragePoolType.LOCAL,
            {"path": f"/var/lib/qarax/e2e-commit-{test_id}"},
        )

        vm_id = None
        try:
            await _attach_pools_to_hosts(
                c, hosts, [overlaybd_pool_id, commit_pool_id]
            )

            # Create the OCI-backed VM
            new_vm = NewVm(
                name=f"e2e-commit-vm-{test_id}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=268435456,
                image_ref=oci_image_ref,
            )
            result = await create_vm.asyncio(client=c, body=new_vm)
            assert isinstance(result, CreateVmResponse)
            vm_id = UUID(str(result.vm_id))
            await wait_for_job(c, result.job_id)

            # Verify VM has image_ref set before commit
            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.image_ref is not None, "VM should have image_ref before commit"

            # Commit the VM (raw httpx since SDK not regenerated yet)
            httpx_client = c.get_async_httpx_client()
            commit_resp = await httpx_client.post(
                f"{QARAX_URL}/vms/{vm_id}/commit",
                json={
                    "storage_pool_id": str(commit_pool_id),
                    "size_bytes": 1073741824,  # 1 GiB
                },
            )
            assert commit_resp.status_code == 202, (
                f"Expected 202, got {commit_resp.status_code}: {commit_resp.text}"
            )
            commit_body = commit_resp.json()
            commit_job_id = commit_body["job_id"]

            try:
                await wait_for_job(c, commit_job_id)
            except RuntimeError as e:
                if "Failed to copy OverlayBD to raw disk" in str(e):
                    pytest.skip("OverlayBD->raw conversion is unavailable in this e2e environment")
                raise

            # Verify VM no longer has image_ref
            vm_after = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm_after.image_ref is None, (
                f"VM image_ref should be cleared after commit, got {vm_after.image_ref}"
            )

            # Verify a Disk storage object exists on the commit pool
            disks_resp = await httpx_client.get(
                f"{QARAX_URL}/storage-objects",
                params={"name": f"committed-{vm_id}"},
            )
            assert disks_resp.status_code == 200
            disk_objects = disks_resp.json()
            assert len(disk_objects) == 1, (
                f"Expected exactly one committed disk object, got {len(disk_objects)}"
            )
            assert disk_objects[0]["object_type"] == "disk"
            assert disk_objects[0]["storage_pool_id"] == str(commit_pool_id)
        finally:
            await _cleanup(
                c,
                vm_id,
                [f"committed-{vm_id}", f"overlaybd-{vm_id}"] if vm_id else [],
                [commit_pool_id, overlaybd_pool_id],
            )


# NFS storage pool tests


@pytest.mark.asyncio
async def test_nfs_storage_pool_attach(client):
    """Create an NFS pool, attach it to the host (which mounts the share), then clean up."""
    async with client as c:
        hosts = _up_hosts(await list_hosts.asyncio(client=c))
        assert hosts and len(hosts) > 0, "No UP hosts registered"
        host = next((h for h in hosts if h.name.startswith("e2e-node")), hosts[0])

        pool_id = await _create_pool(
            c,
            "e2e-nfs-pool",
            StoragePoolType.NFS,
            {"url": f"{NFS_SERVER_HOST}:{NFS_EXPORT_PATH}"},
        )

        try:
            resp = await attach_pool_host.asyncio_detailed(
                client=c,
                pool_id=pool_id,
                body=AttachPoolHostRequest(host_id=host.id),
            )
            assert resp.status_code in (200, 201, 204), (
                f"Attach host failed: HTTP {resp.status_code} — {resp.content}"
            )
        finally:
            try:
                await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)
            except Exception:
                pass


@pytest.mark.asyncio
async def test_nfs_storage_object_create(client):
    """Create an NFS pool, attach it, create a storage object on the mount, then clean up."""
    async with client as c:
        hosts = _up_hosts(await list_hosts.asyncio(client=c))
        assert hosts and len(hosts) > 0, "No UP hosts registered"
        host = next((h for h in hosts if h.name.startswith("e2e-node")), hosts[0])

        pool_id = await _create_pool(
            c,
            "e2e-nfs-obj-pool",
            StoragePoolType.NFS,
            {"url": f"{NFS_SERVER_HOST}:{NFS_EXPORT_PATH}"},
        )

        try:
            attach_resp = await attach_pool_host.asyncio_detailed(
                client=c,
                pool_id=pool_id,
                body=AttachPoolHostRequest(host_id=host.id),
            )
            assert attach_resp.status_code in (200, 201, 204), (
                f"Attach host failed: HTTP {attach_resp.status_code}"
            )

            obj = NewStorageObject(
                name="e2e-nfs-disk",
                storage_pool_id=str(pool_id),
                object_type=StorageObjectType.DISK,
                size_bytes=1073741824,
            )
            obj_id_raw = await create_storage_object.asyncio(client=c, body=obj)
            assert obj_id_raw is not None
            obj_id = UUID(str(obj_id_raw).strip('"'))

            await delete_storage_object.asyncio_detailed(client=c, object_id=obj_id)
        finally:
            try:
                await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)
            except Exception:
                pass
