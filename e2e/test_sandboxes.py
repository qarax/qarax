"""E2E tests for sandbox lifecycle."""

import asyncio
import os
import time
import uuid

import pytest
import pytest_asyncio
from qarax_api_client import Client
from qarax_api_client.api.boot_sources import (
    create as create_boot_source,
)
from qarax_api_client.api.boot_sources import (
    delete as delete_boot_source,
)
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.sandboxes import (
    create as create_sandbox,
)
from qarax_api_client.api.sandboxes import (
    delete as delete_sandbox,
)
from qarax_api_client.api.sandboxes import (
    exec_ as exec_sandbox,
)
from qarax_api_client.api.sandboxes import get as get_sandbox
from qarax_api_client.api.sandboxes import list_ as list_sandboxes
from qarax_api_client.api.storage_objects import delete as delete_storage_object
from qarax_api_client.api.storage_objects import list_ as list_storage_objects
from qarax_api_client.api.storage_pools import (
    attach_host as attach_pool_host,
)
from qarax_api_client.api.storage_pools import (
    create as create_storage_pool,
)
from qarax_api_client.api.storage_pools import (
    delete as delete_storage_pool,
)
from qarax_api_client.api.transfers import create as create_transfer
from qarax_api_client.api.transfers import get as get_transfer
from qarax_api_client.api.vm_templates import (
    create as create_template,
)
from qarax_api_client.api.vm_templates import (
    delete as delete_template,
)
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.models import (
    ExecSandboxRequest,
    Hypervisor,
    NewBootSource,
    NewStoragePool,
    NewTransfer,
    NewVmTemplate,
    StorageObjectType,
    StoragePoolType,
    TransferStatus,
    VmStatus,
)
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest
from qarax_api_client.models.new_sandbox import NewSandbox
from qarax_api_client.models.sandbox_status import SandboxStatus

from helpers import AUTH_HEADERS, call_api, call_api_detailed, up_hosts

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
NFS_SERVER_HOST = os.getenv("NFS_SERVER_HOST", "nfs-server")
NFS_EXPORT_PATH = os.getenv("NFS_EXPORT_PATH", "/nfs-export")
SANDBOX_READY_TIMEOUT = int(os.getenv("SANDBOX_READY_TIMEOUT", "120"))
# Idle timeout + reaper interval (10s) + slack for the VM teardown.
REAPER_GRACE_SECS = 60
TRANSFER_TIMEOUT = 30


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


async def create_bootable_sandbox_template(
    client, hypervisor: str | None, *, shared_pool: bool = False
):
    """Create a bootable sandbox template and return (template_id, cleanup_resources)."""
    test_id = uuid.uuid4().hex[:8]
    hosts = up_hosts(await call_api(list_hosts, client=client))
    assert hosts, "Expected at least one UP host"

    pool_name = f"e2e-sandbox-pool-{test_id}"
    pool_type = StoragePoolType.NFS if shared_pool else StoragePoolType.LOCAL
    pool_config = (
        {"url": f"{NFS_SERVER_HOST}:{NFS_EXPORT_PATH}"}
        if shared_pool
        else {"path": f"/var/lib/qarax/e2e-sandbox-{test_id}"}
    )
    pool_id_raw = await create_storage_pool.asyncio(
        client=client,
        body=NewStoragePool(
            name=pool_name,
            pool_type=pool_type,
            config=pool_config,
        ),
    )
    assert pool_id_raw is not None
    pool_id = uuid.UUID(str(pool_id_raw).strip('"'))

    # A local pool lives on one host; a shared pool is reachable from all of them.
    for host in hosts if shared_pool else hosts[:1]:
        await call_api(
            attach_pool_host,
            client=client,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=host.id),
        )

    transfer = await create_transfer.asyncio(
        client=client,
        pool_id=pool_id,
        body=NewTransfer(
            name=f"e2e-sandbox-kernel-{test_id}",
            source="/var/lib/qarax/images/vmlinux",
            object_type=StorageObjectType.KERNEL,
        ),
    )
    assert transfer is not None

    deadline = time.time() + TRANSFER_TIMEOUT
    while time.time() < deadline:
        transfer = await get_transfer.asyncio(
            client=client, pool_id=pool_id, transfer_id=transfer.id
        )
        assert transfer is not None
        if transfer.status == TransferStatus.COMPLETED:
            break
        if transfer.status == TransferStatus.FAILED:
            raise AssertionError(f"Kernel transfer failed: {transfer.error_message}")
        await asyncio.sleep(0.5)
    else:
        raise TimeoutError("Kernel transfer did not complete in time")

    assert transfer.storage_object_id is not None

    initrd_transfer = await create_transfer.asyncio(
        client=client,
        pool_id=pool_id,
        body=NewTransfer(
            name=f"e2e-sandbox-initrd-{test_id}",
            source="/var/lib/qarax/images/test-initramfs.gz",
            object_type=StorageObjectType.INITRD,
        ),
    )
    assert initrd_transfer is not None
    objects = await list_storage_objects.asyncio(
        client=client, name=f"e2e-sandbox-kernel-{test_id}"
    )
    assert objects is not None
    kernel = next((obj for obj in objects if obj.id == transfer.storage_object_id), None)
    assert kernel is not None, "Expected transferred kernel storage object"

    initrd_deadline = time.time() + TRANSFER_TIMEOUT
    while time.time() < initrd_deadline:
        initrd_transfer = await get_transfer.asyncio(
            client=client, pool_id=pool_id, transfer_id=initrd_transfer.id
        )
        assert initrd_transfer is not None
        if initrd_transfer.status == TransferStatus.COMPLETED:
            break
        if initrd_transfer.status == TransferStatus.FAILED:
            raise AssertionError(f"Initrd transfer failed: {initrd_transfer.error_message}")
        await asyncio.sleep(0.5)
    else:
        raise TimeoutError("Initrd transfer did not complete in time")

    assert initrd_transfer.storage_object_id is not None

    boot_source_name = f"e2e-sandbox-boot-{uuid.uuid4().hex[:8]}"
    boot_source_id_raw = await create_boot_source.asyncio(
        client=client,
        body=NewBootSource(
            name=boot_source_name,
            kernel_image_id=kernel.id,
            kernel_params="console=ttyS0",
            initrd_image_id=initrd_transfer.storage_object_id,
        ),
    )
    assert boot_source_id_raw is not None
    boot_source_id = uuid.UUID(boot_source_id_raw.strip('"'))

    new_template = NewVmTemplate(
        name=f"e2e-sandbox-template-{uuid.uuid4().hex[:8]}",
        hypervisor=hypervisor,
        boot_vcpus=1,
        max_vcpus=1,
        memory_size=256 * 1024 * 1024,
        boot_source_id=boot_source_id,
        boot_mode="kernel",
    )
    template_id_str = await create_template.asyncio(client=client, body=new_template)
    assert template_id_str is not None
    template_id = uuid.UUID(template_id_str.strip('"'))

    return template_id, {
        "template_id": template_id,
        "boot_source_id": boot_source_id,
        "kernel_object_id": transfer.storage_object_id,
        "initrd_object_id": initrd_transfer.storage_object_id,
        "pool_id": pool_id,
    }


async def _delete_tolerating_404(endpoint_module, **kwargs):
    """Delete a resource; a 404 means a test already removed it, anything else fails."""
    response = await call_api_detailed(endpoint_module, **kwargs)
    assert response.status_code in (204, 404), (
        f"{endpoint_module.__name__} failed with HTTP {response.status_code.value}: "
        f"{response.content.decode(errors='replace')}"
    )


async def cleanup_bootable_sandbox_template(client, resources):
    await _delete_tolerating_404(
        delete_template, client=client, vm_template_id=resources["template_id"]
    )
    await _delete_tolerating_404(
        delete_boot_source, client=client, boot_source_id=resources["boot_source_id"]
    )
    await _delete_tolerating_404(
        delete_storage_object, client=client, object_id=str(resources["kernel_object_id"])
    )
    await _delete_tolerating_404(
        delete_storage_object, client=client, object_id=str(resources["initrd_object_id"])
    )
    await _delete_tolerating_404(
        delete_storage_pool, client=client, pool_id=resources["pool_id"]
    )


@pytest_asyncio.fixture(scope="module", loop_scope="module")
async def sandbox_template():
    """Create an explicitly bootable Cloud Hypervisor template for sandbox tests.

    Shared by the module: tests only create sandboxes from it, and the pool
    test removes the sandbox pool it configures.
    """
    async with Client(base_url=QARAX_URL, headers=AUTH_HEADERS) as c:
        template_id, resources = await create_bootable_sandbox_template(c, "cloud_hv")
        yield template_id
        await cleanup_bootable_sandbox_template(c, resources)


async def wait_for_sandbox_status(
    client,
    sandbox_id: uuid.UUID,
    expected: SandboxStatus,
    timeout: int = SANDBOX_READY_TIMEOUT,
) -> object:
    """Poll until sandbox reaches expected status or timeout."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        sandbox = await get_sandbox.asyncio(client=client, sandbox_id=sandbox_id)
        assert sandbox is not None
        if sandbox.status == expected:
            return sandbox
        if sandbox.status == SandboxStatus.ERROR:
            raise AssertionError(
                f"Sandbox {sandbox_id} entered ERROR state while waiting for {expected}: "
                f"{sandbox.error_message}"
            )
        await asyncio.sleep(1)
    sandbox = await get_sandbox.asyncio(client=client, sandbox_id=sandbox_id)
    raise TimeoutError(
        f"Sandbox {sandbox_id} did not reach {expected} within {timeout}s. "
        f"Current status: {sandbox.status if sandbox else 'unknown'}"
    )


async def wait_for_sandbox_pool_ready(client, vm_template_id, min_ready=1, timeout=SANDBOX_READY_TIMEOUT):
    """Poll the sandbox pool endpoint until the requested number of warm members are ready."""
    httpx_client = client.get_async_httpx_client()
    deadline = time.time() + timeout
    while time.time() < deadline:
        response = await httpx_client.get(f"{QARAX_URL}/vm-templates/{vm_template_id}/sandbox-pool")
        assert response.status_code == 200, (
            f"Expected sandbox pool GET to succeed, got {response.status_code}: {response.text}"
        )
        body = response.json()
        if body["current_ready"] >= min_ready:
            return body
        await asyncio.sleep(2)
    raise TimeoutError(
        f"Sandbox pool for template {vm_template_id} did not reach {min_ready} ready member(s)"
    )


@pytest.mark.asyncio
async def test_sandbox_get_returns_details(client, sandbox_template):
    """POST /sandboxes returns ids and a job; GET and list return the new sandbox."""
    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-get-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
            idle_timeout_secs=600,
        )
        resp = await call_api(create_sandbox, client=c, body=req)
        assert resp.job_id is not None

        try:
            sandbox = await call_api(get_sandbox, client=c, sandbox_id=resp.id)
            assert sandbox.id == resp.id
            assert sandbox.vm_id == resp.vm_id
            assert sandbox.vm_template_id == sandbox_template
            assert sandbox.idle_timeout_secs == 600
            assert sandbox.status in (SandboxStatus.PROVISIONING, SandboxStatus.READY)

            sandboxes = await call_api(list_sandboxes, client=c)
            ids = [s.id for s in sandboxes]
            assert resp.id in ids, f"Sandbox {resp.id} not found in list: {ids}"
        finally:
            await call_api(delete_sandbox, client=c, sandbox_id=resp.id)


@pytest.mark.asyncio
async def test_sandbox_delete_removes_vm(client, sandbox_template):
    """DELETE /sandboxes/{id} removes the sandbox and its underlying VM."""
    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-del-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
        )
        resp = await call_api(create_sandbox, client=c, body=req)
        sandbox_id = resp.id
        vm_id = resp.vm_id

        await call_api(delete_sandbox, client=c, sandbox_id=sandbox_id)

        sandbox_resp = await call_api_detailed(get_sandbox, client=c, sandbox_id=sandbox_id)
        assert sandbox_resp.status_code == 404, (
            f"Expected sandbox {sandbox_id} to be deleted, got HTTP {sandbox_resp.status_code}"
        )

        # Underlying VM should be gone too
        vm_resp = await call_api_detailed(get_vm, client=c, vm_id=vm_id)
        assert vm_resp.status_code == 404, (
            f"Expected VM {vm_id} to be deleted, got HTTP {vm_resp.status_code}"
        )


@pytest.mark.asyncio
async def test_sandbox_defaults_to_firecracker_when_template_omits_hypervisor(client):
    """Sandbox VMs default to Firecracker when the template leaves hypervisor unset."""
    async with client as c:
        template_id, resources = await create_bootable_sandbox_template(c, None)
        sandbox_id = None

        try:
            resp = await create_sandbox.asyncio(
                client=c,
                body=NewSandbox(
                    name=f"e2e-sandbox-fc-default-{uuid.uuid4().hex[:8]}",
                    vm_template_id=template_id,
                    idle_timeout_secs=120,
                ),
            )
            assert resp is not None
            sandbox_id = resp.id

            await wait_for_sandbox_status(c, sandbox_id, SandboxStatus.READY)
            vm = await get_vm.asyncio(client=c, vm_id=resp.vm_id)
            assert vm is not None
            assert vm.hypervisor == Hypervisor.FIRECRACKER

            result = await exec_sandbox.asyncio(
                client=c,
                sandbox_id=sandbox_id,
                body=ExecSandboxRequest(
                    command=["/bin/sh", "-c", "printf sandbox-fc-default && uname -s"],
                    timeout_secs=15,
                ),
            )
            assert result is not None
            assert result.exit_code == 0
            assert result.timed_out is False
            assert "sandbox-fc-default" in result.stdout
            assert "Linux" in result.stdout
            assert result.stderr == ""
        finally:
            if sandbox_id is not None:
                await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
            await cleanup_bootable_sandbox_template(c, resources)


@pytest.mark.asyncio
async def test_sandbox_exec_runs_command(client, sandbox_template):
    """POST /sandboxes/{id}/exec executes a command inside the running sandbox."""
    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-exec-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
            idle_timeout_secs=120,
        )
        resp = await call_api(create_sandbox, client=c, body=req)

        try:
            sandbox = await wait_for_sandbox_status(c, resp.id, SandboxStatus.READY)
            # READY means the sandbox VM is running.
            assert sandbox.vm_status == VmStatus.RUNNING, sandbox.vm_status

            result = await exec_sandbox.asyncio(
                client=c,
                sandbox_id=resp.id,
                body=ExecSandboxRequest(
                    command=["/bin/sh", "-c", "printf sandbox-exec && uname -s"],
                    timeout_secs=15,
                ),
            )
            assert result is not None
            assert result.exit_code == 0
            assert result.timed_out is False
            assert "sandbox-exec" in result.stdout
            assert "Linux" in result.stdout
            assert result.stderr == ""
        finally:
            await call_api(delete_sandbox, client=c, sandbox_id=resp.id)


@pytest.mark.asyncio
async def test_sandbox_pool_claim_returns_completed_claim_job(client, sandbox_template):
    """Configured sandbox pools hand out a ready sandbox via the warm-claim path."""
    async with client as c:
        httpx_client = c.get_async_httpx_client()
        configured = None
        created = None

        try:
            response = await httpx_client.put(
                f"{QARAX_URL}/vm-templates/{sandbox_template}/sandbox-pool",
                json={"min_ready": 1},
            )
            assert response.status_code == 200, (
                f"Expected pool configure to succeed, got {response.status_code}: {response.text}"
            )
            configured = response.json()
            assert configured["min_ready"] == 1

            await wait_for_sandbox_pool_ready(c, sandbox_template, min_ready=1)

            created = await create_sandbox.asyncio(
                client=c,
                body=NewSandbox(
                    name=f"e2e-sandbox-pool-claim-{uuid.uuid4().hex[:8]}",
                    vm_template_id=sandbox_template,
                    idle_timeout_secs=120,
                ),
            )
            assert created is not None

            job_response = await httpx_client.get(f"{QARAX_URL}/jobs/{created.job_id}")
            assert job_response.status_code == 200
            job = job_response.json()
            assert job["status"] == "completed"
            assert job["job_type"] == "sandbox_claim"

            sandbox = await wait_for_sandbox_status(
                c, created.id, SandboxStatus.READY, timeout=10
            )
            assert sandbox is not None
            assert sandbox.status == SandboxStatus.READY
        finally:
            if created is not None:
                await delete_sandbox.asyncio_detailed(client=c, sandbox_id=created.id)
            if configured is not None:
                # The template is shared by the module: leave it without a pool.
                response = await httpx_client.delete(
                    f"{QARAX_URL}/vm-templates/{sandbox_template}/sandbox-pool"
                )
                assert response.status_code == 204, response.text


@pytest.mark.asyncio
async def test_sandbox_idle_timeout_reaping(client, sandbox_template):
    """Sandbox with a 1-second idle timeout is reaped by the background reaper."""
    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-reap-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
            idle_timeout_secs=1,
        )
        resp = await call_api(create_sandbox, client=c, body=req)
        sandbox_id = resp.id

        # The reaper (every 10s) deletes READY sandboxes past their idle time,
        # so the sandbox can vanish before a poll ever sees READY. Wait for it
        # to be gone; only ERROR or a timeout fails.
        deadline = time.time() + SANDBOX_READY_TIMEOUT + REAPER_GRACE_SECS
        while True:
            response = await call_api_detailed(get_sandbox, client=c, sandbox_id=sandbox_id)
            if response.status_code == 404:
                break
            assert response.status_code == 200, (
                f"GET sandbox {sandbox_id}: HTTP {response.status_code} {response.content}"
            )
            sandbox = response.parsed
            assert sandbox.status != SandboxStatus.ERROR, (
                f"Sandbox {sandbox_id} entered ERROR: {sandbox.error_message}"
            )
            if time.time() >= deadline:
                await _delete_tolerating_404(delete_sandbox, client=c, sandbox_id=sandbox_id)
                raise AssertionError(
                    f"Sandbox {sandbox_id} was not reaped within "
                    f"{SANDBOX_READY_TIMEOUT + REAPER_GRACE_SECS}s; last status {sandbox.status}"
                )
            await asyncio.sleep(1)

        vm_resp = await call_api_detailed(get_vm, client=c, vm_id=resp.vm_id)
        assert vm_resp.status_code == 404, (
            f"Expected reaped sandbox VM {resp.vm_id} to be deleted, got HTTP "
            f"{vm_resp.status_code}"
        )
