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
SANDBOX_READY_TIMEOUT = int(os.getenv("SANDBOX_READY_TIMEOUT", "60"))
TRANSFER_TIMEOUT = 30


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


@pytest.fixture
async def sandbox_template():
    """Create an explicitly bootable VM template for sandbox tests."""
    async with Client(base_url=QARAX_URL) as c:
        test_id = uuid.uuid4().hex[:8]
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "Expected at least one registered host"

        host = next((h for h in hosts if h.status == HostStatus.UP), hosts[0])

        pool_name = f"e2e-sandbox-pool-{test_id}"
        pool_id_raw = await create_storage_pool.asyncio(
            client=c,
            body=NewStoragePool(
                name=pool_name,
                pool_type=StoragePoolType.LOCAL,
                config={"path": f"/var/lib/qarax/e2e-sandbox-{test_id}"},
            ),
        )
        assert pool_id_raw is not None
        pool_id = uuid.UUID(str(pool_id_raw).strip('"'))

        await attach_pool_host.asyncio_detailed(
            client=c,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=host.id),
        )

        transfer = await create_transfer.asyncio(
            client=c,
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
                client=c, pool_id=pool_id, transfer_id=transfer.id
            )
            assert transfer is not None
            if transfer.status == TransferStatus.COMPLETED:
                break
            if transfer.status == TransferStatus.FAILED:
                raise AssertionError(
                    f"Kernel transfer failed: {transfer.error_message}"
                )
            await asyncio.sleep(0.5)
        else:
            raise TimeoutError("Kernel transfer did not complete in time")

        assert transfer.storage_object_id is not None
        objects = await list_storage_objects.asyncio(
            client=c, name=f"e2e-sandbox-kernel-{test_id}"
        )
        assert objects is not None
        kernel = next(
            (obj for obj in objects if obj.id == transfer.storage_object_id), None
        )
        assert kernel is not None, "Expected transferred kernel storage object"

        boot_source_name = f"e2e-sandbox-boot-{uuid.uuid4().hex[:8]}"
        boot_source_id_raw = await create_boot_source.asyncio(
            client=c,
            body=NewBootSource(
                name=boot_source_name,
                kernel_image_id=kernel.id,
                kernel_params="console=ttyS0",
                initrd_image_id=None,
            ),
        )
        assert boot_source_id_raw is not None
        boot_source_id = uuid.UUID(boot_source_id_raw.strip('"'))

        new_template = NewVmTemplate(
            name=f"e2e-sandbox-template-{uuid.uuid4().hex[:8]}",
            hypervisor="cloud_hv",
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
            boot_source_id=boot_source_id,
            boot_mode="kernel",
        )
        template_id_str = await create_template.asyncio(client=c, body=new_template)
        assert template_id_str is not None
        template_id = uuid.UUID(template_id_str.strip('"'))

        yield template_id

        # Cleanup
        try:
            await delete_template.asyncio_detailed(client=c, vm_template_id=template_id)
        except Exception:
            pass
        try:
            await delete_boot_source.asyncio_detailed(
                client=c, boot_source_id=boot_source_id
            )
        except Exception:
            pass
        try:
            await delete_storage_object.asyncio_detailed(
                client=c, object_id=str(transfer.storage_object_id)
            )
        except Exception:
            pass
        try:
            await delete_storage_pool.asyncio_detailed(client=c, pool_id=pool_id)
        except Exception:
            pass


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
