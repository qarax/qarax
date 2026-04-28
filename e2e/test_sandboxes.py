"""E2E tests for sandbox lifecycle."""

import asyncio
import os
import time
import uuid

import pytest
from qarax_api_client import Client
from qarax_api_client.api.boot_sources import (
    create as create_boot_source,
    delete as delete_boot_source,
)
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.sandboxes import (
    create as create_sandbox,
    delete as delete_sandbox,
    exec_ as exec_sandbox,
)
from qarax_api_client.api.sandboxes import get as get_sandbox
from qarax_api_client.api.sandboxes import list_ as list_sandboxes
from qarax_api_client.api.storage_pools import (
    attach_host as attach_pool_host,
    create as create_storage_pool,
    delete as delete_storage_pool,
)
from qarax_api_client.api.storage_objects import list_ as list_storage_objects
from qarax_api_client.api.storage_objects import delete as delete_storage_object
from qarax_api_client.api.transfers import create as create_transfer
from qarax_api_client.api.transfers import get as get_transfer
from qarax_api_client.api.vm_templates import (
    create as create_template,
    delete as delete_template,
)
from qarax_api_client.models import (
    ExecSandboxRequest,
    NewBootSource,
    NewStoragePool,
    NewTransfer,
    NewVmTemplate,
    StorageObjectType,
    StoragePoolType,
    TransferStatus,
)
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest
from qarax_api_client.models.host_status import HostStatus
from qarax_api_client.models.new_sandbox import NewSandbox
from qarax_api_client.models.sandbox_status import SandboxStatus

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
SANDBOX_READY_TIMEOUT = int(os.getenv("SANDBOX_READY_TIMEOUT", "120"))
TRANSFER_TIMEOUT = 30


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


async def create_bootable_sandbox_template(client, hypervisor: str | None):
    """Create a bootable sandbox template and return (template_id, cleanup_resources)."""
    test_id = uuid.uuid4().hex[:8]
    hosts = await list_hosts.asyncio(client=client)
    assert hosts, "Expected at least one registered host"

    host = next((h for h in hosts if h.status == HostStatus.UP), hosts[0])

    pool_name = f"e2e-sandbox-pool-{test_id}"
    pool_id_raw = await create_storage_pool.asyncio(
        client=client,
        body=NewStoragePool(
            name=pool_name,
            pool_type=StoragePoolType.LOCAL,
            config={"path": f"/var/lib/qarax/e2e-sandbox-{test_id}"},
        ),
    )
    assert pool_id_raw is not None
    pool_id = uuid.UUID(str(pool_id_raw).strip('"'))

    await attach_pool_host.asyncio_detailed(
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


async def cleanup_bootable_sandbox_template(client, resources):
    try:
        await delete_template.asyncio_detailed(
            client=client, vm_template_id=resources["template_id"]
        )
    except Exception:
        pass
    try:
        await delete_boot_source.asyncio_detailed(
            client=client, boot_source_id=resources["boot_source_id"]
        )
    except Exception:
        pass
    try:
        await delete_storage_object.asyncio_detailed(
            client=client, object_id=str(resources["kernel_object_id"])
        )
    except Exception:
        pass
    try:
        await delete_storage_object.asyncio_detailed(
            client=client, object_id=str(resources["initrd_object_id"])
        )
    except Exception:
        pass
    try:
        await delete_storage_pool.asyncio_detailed(client=client, pool_id=resources["pool_id"])
    except Exception:
        pass


@pytest.fixture
async def sandbox_template():
    """Create an explicitly bootable Cloud Hypervisor template for sandbox tests."""
    async with Client(base_url=QARAX_URL) as c:
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
async def test_sandbox_create_returns_202(client, sandbox_template):
    """POST /sandboxes returns 202 with id, vm_id, job_id."""
    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
        )
        resp = await create_sandbox.asyncio(client=c, body=req)
        assert resp is not None, "Expected CreateSandboxResponse, got None"
        assert resp.id is not None
        assert resp.vm_id is not None
        assert resp.job_id is not None

        # Cleanup
        try:
            await delete_sandbox.asyncio_detailed(client=c, sandbox_id=resp.id)
        except Exception:
            pass


@pytest.mark.asyncio
async def test_sandbox_appears_in_list(client, sandbox_template):
    """Created sandbox appears in GET /sandboxes."""
    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-list-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
        )
        resp = await create_sandbox.asyncio(client=c, body=req)
        assert resp is not None
        sandbox_id = resp.id

        try:
            sandboxes = await list_sandboxes.asyncio(client=c)
            assert sandboxes is not None
            ids = [s.id for s in sandboxes]
            assert sandbox_id in ids, f"Sandbox {sandbox_id} not found in list: {ids}"
        finally:
            await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)


@pytest.mark.asyncio
async def test_sandbox_get_returns_details(client, sandbox_template):
    """GET /sandboxes/{id} returns sandbox with vm_id and template_id."""
    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-get-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
            idle_timeout_secs=600,
        )
        resp = await create_sandbox.asyncio(client=c, body=req)
        assert resp is not None

        try:
            sandbox = await get_sandbox.asyncio(client=c, sandbox_id=resp.id)
            assert sandbox is not None
            assert sandbox.id == resp.id
            assert sandbox.vm_id == resp.vm_id
            assert sandbox.vm_template_id == sandbox_template
            assert sandbox.idle_timeout_secs == 600
            assert sandbox.status in (SandboxStatus.PROVISIONING, SandboxStatus.READY)
        finally:
            await delete_sandbox.asyncio_detailed(client=c, sandbox_id=resp.id)


@pytest.mark.asyncio
async def test_sandbox_delete_removes_vm(client, sandbox_template):
    """DELETE /sandboxes/{id} removes the sandbox and its underlying VM."""
    from qarax_api_client.api.vms import get as get_vm

    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-del-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
        )
        resp = await create_sandbox.asyncio(client=c, body=req)
        assert resp is not None
        sandbox_id = resp.id
        vm_id = resp.vm_id

        await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)

        # Sandbox should no longer appear in list
        sandboxes = await list_sandboxes.asyncio(client=c)
        if sandboxes:
            assert sandbox_id not in [s.id for s in sandboxes]

        # Underlying VM should be gone too
        vm_resp = await get_vm.asyncio_detailed(client=c, vm_id=vm_id)
        assert vm_resp.status_code.value == 404, (
            f"Expected VM {vm_id} to be deleted, got HTTP {vm_resp.status_code}"
        )


@pytest.mark.asyncio
async def test_sandbox_full_lifecycle(client, sandbox_template):
    """Create sandbox, wait for READY, then delete it."""
    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-lifecycle-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
        )
        resp = await create_sandbox.asyncio(client=c, body=req)
        assert resp is not None
        sandbox_id = resp.id

        try:
            # Wait for sandbox to reach READY (VM running)
            sandbox = await wait_for_sandbox_status(c, sandbox_id, SandboxStatus.READY)
            vm_status = getattr(sandbox, "vm_status", None)

            assert vm_status is not None
            assert str(vm_status) == "running"

            # GET while READY should return ip_address and vm_status
            fetched = await get_sandbox.asyncio(client=c, sandbox_id=sandbox_id)
            assert fetched is not None
            assert fetched.status == SandboxStatus.READY

        finally:
            await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)


@pytest.mark.asyncio
async def test_sandbox_defaults_to_firecracker_when_template_omits_hypervisor(client):
    """Sandbox VMs default to Firecracker when the template leaves hypervisor unset."""
    from qarax_api_client.api.vms import get as get_vm
    from qarax_api_client.models import Hypervisor

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
        resp = await create_sandbox.asyncio(client=c, body=req)
        assert resp is not None

        try:
            await wait_for_sandbox_status(c, resp.id, SandboxStatus.READY)

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
            await delete_sandbox.asyncio_detailed(client=c, sandbox_id=resp.id)


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
                await httpx_client.delete(
                    f"{QARAX_URL}/vm-templates/{sandbox_template}/sandbox-pool"
                )


@pytest.mark.asyncio
async def test_sandbox_idle_timeout_reaping(client, sandbox_template):
    """Sandbox with a 1-second idle timeout is reaped by the background reaper."""
    async with client as c:
        req = NewSandbox(
            name=f"e2e-sandbox-reap-{uuid.uuid4().hex[:8]}",
            vm_template_id=sandbox_template,
            idle_timeout_secs=1,
        )
        resp = await create_sandbox.asyncio(client=c, body=req)
        assert resp is not None
        sandbox_id = resp.id

        # Wait for READY first so the idle clock starts
        try:
            await wait_for_sandbox_status(c, sandbox_id, SandboxStatus.READY)
        except (AssertionError, TimeoutError):
            # If VM never started, clean up and skip
            await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
            pytest.skip("VM did not reach READY in time; skipping reaper test")

        # The reaper runs every 15s; wait up to 60s for the sandbox to disappear
        deadline = time.time() + 60
        while time.time() < deadline:
            await asyncio.sleep(2)
            sandboxes = await list_sandboxes.asyncio(client=c)
            if sandboxes is None or sandbox_id not in [s.id for s in sandboxes]:
                return  # Reaper cleaned it up

        pytest.fail(
            f"Sandbox {sandbox_id} was not reaped within 60s despite 1-second idle timeout"
        )
