"""E2E tests for sandbox lifecycle."""

import asyncio
import contextlib
import os
import re
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
from qarax_api_client.api.hosts import resources as host_resources
from qarax_api_client.api.sandboxes import (
    create as create_sandbox,
)
from qarax_api_client.api.sandboxes import (
    delete as delete_sandbox,
)
from qarax_api_client.api.sandboxes import (
    exec_ as exec_sandbox,
)
from qarax_api_client.api.sandboxes import fork as fork_sandbox
from qarax_api_client.api.sandboxes import get as get_sandbox
from qarax_api_client.api.sandboxes import list_ as list_sandboxes
from qarax_api_client.api.storage_objects import create as create_storage_object
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
from qarax_api_client.api.vms import exec_ as exec_vm
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.api.vms import list_nics
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.api.vms import stop as stop_vm
from qarax_api_client.models import (
    ExecSandboxRequest,
    ExecVmRequest,
    ForkSandboxRequest,
    Hypervisor,
    NewBootSource,
    NewStorageObject,
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

from helpers import (
    AUTH_HEADERS,
    NODE_CONTAINER_PREFIX,
    REGISTRY_INTERNAL_URL,
    attach_overlaybd_pool,
    call_api,
    call_api_detailed,
    containers,
    create_sparse_file_on_nodes,
    docker,
    exec_when_ready,
    isolated_network,
    node_storage_diagnostics,
    overlaybd_disk_wwids,
    remove_file_from_nodes,
    up_hosts,
    vm_diagnostics,
    wait_for_start,
)

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
    client,
    hypervisor: str | None,
    *,
    shared_pool: bool = False,
    cloud_init_user_data: str | None = None,
    root_disk_bytes: int | None = None,
):
    """Create a bootable sandbox template and return (template_id, cleanup_resources).

    With `root_disk_bytes`, its VMs get a blank raw disk of that size, /dev/vda,
    in the template's local pool: they still boot the test initramfs."""
    test_id = uuid.uuid4().hex[:8]
    hosts = up_hosts((await call_api(list_hosts, client=client)).items)
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
    kernel = next(
        (obj for obj in objects.items if obj.id == transfer.storage_object_id), None
    )
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

    root_disk = None
    if root_disk_bytes is not None:
        assert not shared_pool, "the root disk is a file in a local pool"
        root_disk_path = f"{pool_config['path']}/e2e-root-{test_id}.raw"
        create_sparse_file_on_nodes(root_disk_path, root_disk_bytes)
        root_disk_id_raw = await call_api(
            create_storage_object,
            client=client,
            body=NewStorageObject(
                name=f"e2e-sandbox-root-{test_id}",
                storage_pool_id=pool_id,
                object_type=StorageObjectType.DISK,
                size_bytes=root_disk_bytes,
                config={"path": root_disk_path},
            ),
        )
        root_disk = (uuid.UUID(str(root_disk_id_raw).strip('"')), root_disk_path)

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
        cloud_init_user_data=cloud_init_user_data,
        root_disk_object_id=root_disk[0] if root_disk else None,
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
        "root_disk": root_disk,
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
    if resources["root_disk"] is not None:
        root_disk_id, root_disk_path = resources["root_disk"]
        await _delete_tolerating_404(
            delete_storage_object, client=client, object_id=str(root_disk_id)
        )
        remove_file_from_nodes(root_disk_path)
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


@contextlib.asynccontextmanager
async def bootable_sandbox_template(c, hypervisor, **options):
    """create_bootable_sandbox_template as a context manager: yields the
    template's UUID, and cleans up after the test's own cleanup."""
    template_id, resources = await create_bootable_sandbox_template(c, hypervisor, **options)
    try:
        yield template_id
    finally:
        await cleanup_bootable_sandbox_template(c, resources)


@contextlib.asynccontextmanager
async def oci_sandbox_template(c, hypervisor, image_ref):
    """A template of sandboxes that boot `image_ref` on `hypervisor`, with an
    OverlayBD storage pool on each UP host that takes one. Yields the
    template's UUID; deletes it and the pool after the test's own cleanup."""
    test_id = uuid.uuid4().hex[:8]
    hosts = up_hosts((await call_api(list_hosts, client=c)).items)
    pool_id = uuid.UUID(
        str(
            await call_api(
                create_storage_pool,
                client=c,
                body=NewStoragePool(
                    name=f"e2e-sandbox-obd-{test_id}",
                    pool_type=StoragePoolType.OVERLAY_BD,
                    config={"url": f"http://{REGISTRY_INTERNAL_URL}"},
                ),
            )
        ).strip('"')
    )
    template_id = None
    try:
        await attach_overlaybd_pool(c, hosts, pool_id)
        template_id = uuid.UUID(
            str(
                await call_api(
                    create_template,
                    client=c,
                    body=NewVmTemplate(
                        name=f"e2e-sandbox-oci-{test_id}",
                        hypervisor=hypervisor,
                        boot_vcpus=1,
                        max_vcpus=1,
                        memory_size=256 * 1024 * 1024,
                        image_ref=image_ref,
                    ),
                )
            ).strip('"')
        )
        yield template_id
    finally:
        if template_id is not None:
            await delete_template.asyncio_detailed(client=c, vm_template_id=template_id)
        await delete_storage_pool.asyncio_detailed(client=c, pool_id=pool_id)


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


async def wait_for_sandbox_pool(client, vm_template_id, done, what, timeout=SANDBOX_READY_TIMEOUT):
    """Poll the template's sandbox pool until done(pool) holds, and return it;
    `what` names the condition for the timeout's message."""
    httpx_client = client.get_async_httpx_client()
    deadline = time.time() + timeout
    body = None
    while time.time() < deadline:
        response = await httpx_client.get(f"{QARAX_URL}/v1/vm-templates/{vm_template_id}/sandbox-pool")
        assert response.status_code == 200, (
            f"Expected sandbox pool GET to succeed, got {response.status_code}: {response.text}"
        )
        body = response.json()
        if done(body):
            return body
        await asyncio.sleep(2)
    raise TimeoutError(f"Sandbox pool for template {vm_template_id} did not reach {what}: {body}")


async def wait_for_sandbox_pool_ready(client, vm_template_id, min_ready=1, timeout=SANDBOX_READY_TIMEOUT):
    """Poll the sandbox pool endpoint until the requested number of warm members are ready."""
    return await wait_for_sandbox_pool(
        client,
        vm_template_id,
        lambda pool: pool["current_ready"] >= min_ready,
        f"{min_ready} ready member(s)",
        timeout,
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

            sandboxes = (await call_api(list_sandboxes, client=c)).items
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


@pytest.mark.parametrize("hypervisor", ["firecracker", "cloud_hv"])
@pytest.mark.asyncio
async def test_sandbox_fork_continues_from_its_source(client, hypervisor):
    """A fork continues from its source's state, files and running processes
    included, with a random generator of its own; afterwards each one's
    changes are its own."""
    # The test initramfs symlinks only some busybox applets.
    random_bytes = "/bin/busybox head -c 16 /dev/urandom | /bin/busybox md5sum"
    counter = (
        "/bin/busybox setsid /bin/sh -c "
        "'i=0; while :; do i=$((i+1)); echo $i > /root/counter; sleep 0.1; done' "
        "</dev/null >/dev/null 2>&1 &"
    )

    async with client as c:
        template_id, resources = await create_bootable_sandbox_template(c, hypervisor)
        sandbox_ids = []

        async def sh(sandbox_id, script):
            result = await call_api(
                exec_sandbox,
                client=c,
                sandbox_id=sandbox_id,
                body=ExecSandboxRequest(command=["/bin/sh", "-c", script], timeout_secs=15),
            )
            assert result.exit_code == 0, result.stderr
            return result.stdout.strip()

        try:
            source = await call_api(
                create_sandbox,
                client=c,
                body=NewSandbox(
                    name=f"e2e-sandbox-fork-{uuid.uuid4().hex[:8]}",
                    vm_template_id=template_id,
                    idle_timeout_secs=300,
                ),
            )
            sandbox_ids.append(source.id)
            await wait_for_sandbox_status(c, source.id, SandboxStatus.READY)
            await sh(source.id, f"echo source > /root/state && {counter}")

            forks = await call_api(
                fork_sandbox,
                client=c,
                sandbox_id=source.id,
                body=ForkSandboxRequest(count=2),
            )
            sandbox_ids.extend(fork.id for fork in forks)
            assert len(forks) == 2
            for fork in forks:
                assert fork.status == SandboxStatus.READY, fork.status
                assert fork.vm_status == VmStatus.RUNNING, fork.vm_status

            randoms = set()
            for fork in forks:
                assert await sh(fork.id, "cat /root/state") == "source"
                before = int(await sh(fork.id, "cat /root/counter"))
                await asyncio.sleep(1)
                after = int(await sh(fork.id, "cat /root/counter"))
                assert after > before, "the source's counting process didn't continue in the fork"
                randoms.add(await sh(fork.id, random_bytes))
                await sh(fork.id, f"echo {fork.id} > /root/state")
            randoms.add(await sh(source.id, random_bytes))
            assert len(randoms) == 3, "forks share their source's random generator"

            assert await sh(source.id, "cat /root/state") == "source"
            for fork in forks:
                assert await sh(fork.id, "cat /root/state") == str(fork.id)
        finally:
            for sandbox_id in reversed(sandbox_ids):
                await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
            await cleanup_bootable_sandbox_template(c, resources)


@pytest.mark.parametrize("hypervisor", ["firecracker", "cloud_hv"])
@pytest.mark.asyncio
async def test_sandbox_fork_is_charged_the_memory_it_holds_of_its_own(client, hypervisor):
    """A fork shares its source's memory: its host is charged what it holds of
    its own, as its node measures it, plus headroom, not its memory size."""
    mib = 1024 * 1024
    # scheduling.fork_memory_headroom_ratio of the template's 256 MiB.
    headroom = 64 * mib

    async with client as c:
        template_id, resources = await create_bootable_sandbox_template(c, hypervisor)
        sandbox_ids = []

        async def allocated(host_id):
            capacity = await call_api(host_resources, client=c, host_id=host_id)
            return capacity.allocated_memory_bytes

        try:
            source = await call_api(
                create_sandbox,
                client=c,
                body=NewSandbox(
                    name=f"e2e-sandbox-fork-mem-{uuid.uuid4().hex[:8]}",
                    vm_template_id=template_id,
                    idle_timeout_secs=300,
                ),
            )
            sandbox_ids.append(source.id)
            await wait_for_sandbox_status(c, source.id, SandboxStatus.READY)
            host_id = (await call_api(get_vm, client=c, vm_id=source.vm_id)).host_id

            before = await allocated(host_id)
            forks = await call_api(
                fork_sandbox,
                client=c,
                sandbox_id=source.id,
                body=ForkSandboxRequest(count=2),
            )
            sandbox_ids.extend(fork.id for fork in forks)
            # The forks' headroom, and what their node may have measured of
            # them since: a few MiB each, not 256.
            forked = await allocated(host_id)
            assert 2 * headroom <= forked - before < 2 * (headroom + 32 * mib), forked - before

            # What a fork writes is its own, and charged once its node has
            # measured it (the resource monitor polls every 30 s).
            result = await call_api(
                exec_sandbox,
                client=c,
                sandbox_id=forks[0].id,
                body=ExecSandboxRequest(
                    command=[
                        "/bin/busybox",
                        "dd",
                        "if=/dev/zero",
                        "of=/root/fill",
                        "bs=1M",
                        "count=96",
                    ],
                    timeout_secs=30,
                ),
            )
            assert result.exit_code == 0, result.stderr
            deadline = time.time() + 75
            while (grown := await allocated(host_id) - forked) < 90 * mib:
                assert time.time() < deadline, (
                    f"the fork's charge grew by {grown // mib} MiB, not by what it wrote"
                )
                await asyncio.sleep(2)
        finally:
            for sandbox_id in reversed(sandbox_ids):
                await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
            await cleanup_bootable_sandbox_template(c, resources)


@pytest.mark.asyncio
async def test_sandbox_fork_of_an_oci_sandbox_forks_its_disk(client, bootable_oci_image_ref):
    """A fork of a sandbox booted from an OCI image gets the image with the
    sandbox's changes as they were at the fork: synced files, a write still in
    the page cache, a running process. Afterwards each one's disk is its own."""
    test_id = uuid.uuid4().hex[:8]
    counter = (
        "setsid sh -c 'i=0; while :; do i=$((i+1)); echo $i > /root/counter; sleep 0.1; done' "
        "</dev/null >/dev/null 2>&1 &"
    )

    async with client as c:
        source_vm_id = None
        sandbox_ids = []

        async def sh(sandbox_id, script):
            result = await call_api(
                exec_sandbox,
                client=c,
                sandbox_id=sandbox_id,
                body=ExecSandboxRequest(command=["/bin/sh", "-c", script], timeout_secs=15),
            )
            assert result.exit_code == 0, result.stderr
            return result.stdout.strip()

        async with oci_sandbox_template(
            c, Hypervisor.FIRECRACKER, bootable_oci_image_ref
        ) as template_id:
            try:
                source = await call_api(
                    create_sandbox,
                    client=c,
                    body=NewSandbox(
                        name=f"e2e-fork-oci-{test_id}",
                        vm_template_id=template_id,
                        idle_timeout_secs=300,
                    ),
                )
                sandbox_ids.append(source.id)
                source_vm_id = source.vm_id
                # READY means its guest agent answers, however long the image's
                # guest takes to start it: exec works at once.
                await wait_for_sandbox_status(c, source.id, SandboxStatus.READY)
                await sh(
                    source.id,
                    f"echo source > /root/state && sync && echo unsynced > /root/unsynced && {counter}",
                )

                response = await call_api_detailed(
                    fork_sandbox,
                    client=c,
                    sandbox_id=source.id,
                    body=ForkSandboxRequest(count=2),
                )
                assert response.status_code == 201, (
                    f"{response.content.decode(errors='replace')}\n{vm_diagnostics(source_vm_id)}"
                )
                forks = response.parsed
                sandbox_ids.extend(fork.id for fork in forks)
                # Each disk has a SCSI identity of its own, or the host's
                # multipathd takes them for paths to one LUN and holds them.
                wwids = overlaybd_disk_wwids()
                assert len(wwids) >= 3 and len(set(wwids.values())) == len(wwids), (
                    f"{wwids}\n{node_storage_diagnostics()}"
                )
                # After the fork point: no fork may see it.
                await sh(source.id, "echo after > /root/after && sync")

                for fork in forks:
                    # Read from the disk, not the page cache the fork inherited.
                    await sh(fork.id, "echo 3 > /proc/sys/vm/drop_caches")
                    assert await sh(fork.id, "cat /root/state") == "source"
                    assert await sh(fork.id, "cat /root/unsynced") == "unsynced"
                    assert await sh(fork.id, "[ -e /root/after ] && echo yes || echo no") == "no"
                    before = int(await sh(fork.id, "cat /root/counter"))
                    await asyncio.sleep(1)
                    assert int(await sh(fork.id, "cat /root/counter")) > before, (
                        "the source's counting process didn't continue in the fork"
                    )
                    await sh(fork.id, f"echo {fork.id} > /root/state && sync")

                assert await sh(source.id, "cat /root/state") == "source"
                for fork in forks:
                    await sh(fork.id, "echo 3 > /proc/sys/vm/drop_caches")
                    assert await sh(fork.id, "cat /root/state") == str(fork.id)

                # Each sandbox has an image object of its own, which goes with it.
                vm_ids = [source_vm_id] + [fork.vm_id for fork in forks]
                for vm_id in vm_ids:
                    objects = await call_api(
                        list_storage_objects, client=c, name=f"overlaybd-{vm_id}"
                    )
                    assert [obj.owner_vm_id for obj in objects.items] == [vm_id], objects.items
                for sandbox_id in reversed(sandbox_ids):
                    await call_api(delete_sandbox, client=c, sandbox_id=sandbox_id)
                sandbox_ids.clear()
                for vm_id in vm_ids:
                    objects = await call_api(
                        list_storage_objects, client=c, name=f"overlaybd-{vm_id}"
                    )
                    assert not objects.items, f"VM {vm_id}'s image object outlived it"
            finally:
                for sandbox_id in reversed(sandbox_ids):
                    await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)


@pytest.mark.parametrize("hypervisor", ["firecracker", "cloud_hv"])
@pytest.mark.asyncio
async def test_sandbox_fork_on_a_network_gets_addresses_of_its_own(client, hypervisor):
    """Each fork of a sandbox on a network has a NIC of its own there: its
    guest takes the fork's MAC and address in place of its source's, keeps its
    default route, and the source reaches it."""
    test_id = uuid.uuid4().hex[:8]
    gateway = "10.95.0.1"

    async with client as c:
        hosts = up_hosts((await call_api(list_hosts, client=c)).items)

        async def sh(sandbox_id, script):
            result = await call_api(
                exec_sandbox,
                client=c,
                sandbox_id=sandbox_id,
                body=ExecSandboxRequest(command=["/bin/sh", "-c", script], timeout_secs=15),
            )
            assert result.exit_code == 0, f"{script}: {result.stdout} {result.stderr}"
            return result.stdout

        async def guest_nic(sandbox_id):
            """eth0's MAC, IPv4 address and default gateway, in the guest."""
            out = await sh(
                sandbox_id,
                "cat /sys/class/net/eth0/address; ip addr show dev eth0; ip route",
            )
            address = re.search(r"inet (\d+\.\d+\.\d+\.\d+)/", out)
            default = re.search(r"default via (\S+)", out)
            return (
                out.splitlines()[0].strip(),
                address and address.group(1),
                default and default.group(1),
            )

        async def api_address(vm_id):
            nics = await call_api(list_nics, client=c, vm_id=vm_id)
            assert len(nics) == 1, nics
            return nics[0].ip_address.split("/")[0]

        async with isolated_network(c, test_id, hosts, 95) as network_id:
            template_id, resources = await create_bootable_sandbox_template(c, hypervisor)
            sandbox_ids = []
            try:
                source = await call_api(
                    create_sandbox,
                    client=c,
                    body=NewSandbox(
                        name=f"e2e-fork-nic-{test_id}",
                        vm_template_id=template_id,
                        idle_timeout_secs=300,
                        network_id=network_id,
                    ),
                )
                sandbox_ids.append(source.id)
                await wait_for_sandbox_status(c, source.id, SandboxStatus.READY)
                source_mac, source_address, source_gateway = await guest_nic(source.id)
                assert (source_address, source_gateway) == (
                    await api_address(source.vm_id),
                    gateway,
                )

                response = await call_api_detailed(
                    fork_sandbox,
                    client=c,
                    sandbox_id=source.id,
                    body=ForkSandboxRequest(count=2),
                )
                assert response.status_code == 201, response.content.decode()
                forks = response.parsed
                sandbox_ids.extend(fork.id for fork in forks)

                macs, addresses = [source_mac], [source_address]
                for fork in forks:
                    mac, address, default = await guest_nic(fork.id)
                    assert address == await api_address(fork.vm_id), (fork.id, address)
                    assert default == gateway, (fork.id, default)
                    macs.append(mac)
                    addresses.append(address)
                assert len(set(macs)) == 3 and len(set(addresses)) == 3, (macs, addresses)

                # On the network as themselves: the source reaches each fork,
                # each fork its gateway.
                for fork, address in zip(forks, addresses[1:]):
                    await sh(source.id, f"ping -c 1 -W 3 {address}")
                    await sh(fork.id, f"ping -c 1 -W 3 {gateway}")
            finally:
                # Synchronous: the network can go once the sandboxes are gone.
                for sandbox_id in sandbox_ids:
                    await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
                await cleanup_bootable_sandbox_template(c, resources)


@pytest.mark.asyncio
async def test_sandbox_fork_with_cloud_init_has_a_seed_of_its_own(client):
    """A fork of a sandbox with cloud-init gets a seed image of its own: it
    boots again after its source, and the source's seed, are gone."""
    async with client as c:
        template_id, resources = await create_bootable_sandbox_template(
            c, "firecracker", cloud_init_user_data="#cloud-config\n"
        )
        sandbox_ids = []
        try:
            source = await call_api(
                create_sandbox,
                client=c,
                body=NewSandbox(
                    name=f"e2e-fork-ci-{uuid.uuid4().hex[:8]}",
                    vm_template_id=template_id,
                    idle_timeout_secs=300,
                ),
            )
            sandbox_ids.append(source.id)
            await wait_for_sandbox_status(c, source.id, SandboxStatus.READY)

            response = await call_api_detailed(
                fork_sandbox,
                client=c,
                sandbox_id=source.id,
                body=ForkSandboxRequest(count=1),
            )
            assert response.status_code == 201, response.content.decode()
            [fork] = response.parsed
            sandbox_ids.append(fork.id)

            await call_api(delete_sandbox, client=c, sandbox_id=source.id)
            sandbox_ids.remove(source.id)

            # A cold boot opens the seed again: the fork's own.
            await call_api(stop_vm, client=c, vm_id=fork.vm_id)
            started = await call_api(start_vm, client=c, vm_id=fork.vm_id)
            await wait_for_start(c, fork.vm_id, started.job_id)
            result = await exec_when_ready(c, fork.vm_id, "cat /sys/block/vda/size")
            assert result.exit_code == 0 and int(result.stdout) > 0, result
        finally:
            for sandbox_id in sandbox_ids:
                await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
            await cleanup_bootable_sandbox_template(c, resources)


@pytest.mark.asyncio
async def test_sandbox_fork_copies_its_data_disk(client):
    """Each fork of a sandbox with a writable disk gets a copy of it as it was,
    its own: what the source writes after the fork, or a fork writes, the
    others don't see. The copies go with the forks."""
    read = "echo 3 > /proc/sys/vm/drop_caches; /bin/busybox dd if=/dev/vda bs=8 count=1 2>/dev/null"

    def write(text):
        return f"printf {text} | /bin/busybox dd of=/dev/vda bs=8 count=1 conv=fsync 2>/dev/null"

    async with client as c:
        template_id, resources = await create_bootable_sandbox_template(
            c, "firecracker", root_disk_bytes=64 * 1024 * 1024
        )
        sandbox_ids = []

        async def sh(sandbox_id, script):
            result = await call_api(
                exec_sandbox,
                client=c,
                sandbox_id=sandbox_id,
                body=ExecSandboxRequest(command=["/bin/sh", "-c", script], timeout_secs=15),
            )
            assert result.exit_code == 0, f"{script}: {result.stdout} {result.stderr}"
            return result.stdout.strip()

        async def disks_owned_by(vm_ids):
            objects = await call_api(
                list_storage_objects, client=c, pool_id=resources["pool_id"]
            )
            return sorted(
                str(obj.owner_vm_id) for obj in objects.items if obj.owner_vm_id in vm_ids
            )

        try:
            source = await call_api(
                create_sandbox,
                client=c,
                body=NewSandbox(
                    name=f"e2e-fork-disk-{uuid.uuid4().hex[:8]}",
                    vm_template_id=template_id,
                    idle_timeout_secs=300,
                ),
            )
            sandbox_ids.append(source.id)
            await wait_for_sandbox_status(c, source.id, SandboxStatus.READY)
            await sh(source.id, write("source--"))

            response = await call_api_detailed(
                fork_sandbox,
                client=c,
                sandbox_id=source.id,
                body=ForkSandboxRequest(count=2),
            )
            assert response.status_code == 201, response.content.decode()
            forks = response.parsed
            sandbox_ids.extend(fork.id for fork in forks)

            await sh(source.id, write("after---"))
            for i, fork in enumerate(forks):
                assert await sh(fork.id, read) == "source--"
                await sh(fork.id, write(f"fork-{i}--"))
            assert await sh(source.id, read) == "after---"
            for i, fork in enumerate(forks):
                assert await sh(fork.id, read) == f"fork-{i}--"

            fork_vm_ids = [fork.vm_id for fork in forks]
            assert await disks_owned_by(fork_vm_ids) == sorted(map(str, fork_vm_ids))
            for fork in forks:
                await call_api(delete_sandbox, client=c, sandbox_id=fork.id)
                sandbox_ids.remove(fork.id)
            assert await disks_owned_by(fork_vm_ids) == []
        finally:
            for sandbox_id in sandbox_ids:
                await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
            await cleanup_bootable_sandbox_template(c, resources)


@pytest.mark.asyncio
async def test_a_sandbox_forked_again_forks_from_a_diff(client):
    """Forking a sandbox again writes only the memory it dirtied since its last
    fork: the second round's fork has what the source did in between, the
    first round's doesn't."""
    async with client as c:
        template_id, resources = await create_bootable_sandbox_template(c, "firecracker")
        sandbox_ids = []

        async def sh(sandbox_id, script):
            result = await call_api(
                exec_sandbox,
                client=c,
                sandbox_id=sandbox_id,
                body=ExecSandboxRequest(command=["/bin/sh", "-c", script], timeout_secs=15),
            )
            assert result.exit_code == 0, f"{script}: {result.stdout} {result.stderr}"
            return result.stdout.strip()

        async def fork_once(sandbox_id):
            response = await call_api_detailed(
                fork_sandbox,
                client=c,
                sandbox_id=sandbox_id,
                body=ForkSandboxRequest(count=1),
            )
            assert response.status_code == 201, response.content.decode()
            [fork] = response.parsed
            sandbox_ids.append(fork.id)
            return fork

        try:
            source = await call_api(
                create_sandbox,
                client=c,
                body=NewSandbox(
                    name=f"e2e-fork-diff-{uuid.uuid4().hex[:8]}",
                    vm_template_id=template_id,
                    idle_timeout_secs=300,
                ),
            )
            sandbox_ids.append(source.id)
            await wait_for_sandbox_status(c, source.id, SandboxStatus.READY)

            await sh(source.id, "echo one > /root/state")
            first = await fork_once(source.id)
            await sh(source.id, "echo two > /root/state")
            second = await fork_once(source.id)

            assert await sh(first.id, "cat /root/state") == "one"
            assert await sh(second.id, "cat /root/state") == "two"
            assert await sh(source.id, "cat /root/state") == "two"
            logs = "".join(
                docker("logs", container, check=False).stdout
                for container in containers(NODE_CONTAINER_PREFIX)
            )
            assert f"Forking VM {source.vm_id} from a diff snapshot" in logs
        finally:
            for sandbox_id in reversed(sandbox_ids):
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
                f"{QARAX_URL}/v1/vm-templates/{sandbox_template}/sandbox-pool",
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

            job_response = await httpx_client.get(f"{QARAX_URL}/v1/jobs/{created.job_id}")
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
                    f"{QARAX_URL}/v1/vm-templates/{sandbox_template}/sandbox-pool"
                )
                assert response.status_code == 204, response.text


@pytest.mark.asyncio
async def test_an_oci_sandbox_pool_hands_out_members_that_booted_its_image(
    client, bootable_oci_image_ref
):
    """A pool of a template that boots an OCI image keeps members that pulled
    and booted it: a claim gets one that runs the image at once."""
    async with client as c:
        httpx_client = c.get_async_httpx_client()
        async with oci_sandbox_template(
            c, Hypervisor.FIRECRACKER, bootable_oci_image_ref
        ) as template_id:
            pool_url = f"{QARAX_URL}/v1/vm-templates/{template_id}/sandbox-pool"
            created = None
            try:
                response = await httpx_client.put(pool_url, json={"min_ready": 1})
                assert response.status_code == 200, response.text
                await wait_for_sandbox_pool_ready(c, template_id, min_ready=1)

                created = await call_api(
                    create_sandbox,
                    client=c,
                    body=NewSandbox(
                        name=f"e2e-oci-pool-{uuid.uuid4().hex[:8]}",
                        vm_template_id=template_id,
                        idle_timeout_secs=300,
                    ),
                )
                sandbox = await call_api(get_sandbox, client=c, sandbox_id=created.id)
                assert sandbox.status == SandboxStatus.READY, sandbox.status
                result = await call_api(
                    exec_sandbox,
                    client=c,
                    sandbox_id=created.id,
                    body=ExecSandboxRequest(command=["cat", "/etc/os-release"], timeout_secs=15),
                )
                assert result.exit_code == 0, result.stderr
                assert "Alpine" in result.stdout, result.stdout
            finally:
                if created is not None:
                    await delete_sandbox.asyncio_detailed(client=c, sandbox_id=created.id)
                # Deleting the pool destroys its members.
                response = await httpx_client.delete(pool_url)
                assert response.status_code in (204, 404), response.text


@pytest.mark.parametrize(
    "image, hypervisor",
    [(False, "firecracker"), (True, "firecracker"), (True, "cloud_hv")],
    ids=["kernel-firecracker", "oci-firecracker", "oci-cloud_hv"],
)
@pytest.mark.asyncio
async def test_a_forking_sandbox_pool_fills_with_forks_of_its_seed(
    client, bootable_oci_image_ref, image, hypervisor
):
    """A pool that forks keeps one booted VM of its template, its seed, and
    makes its members by forking it: a member continues from the seed's state
    as it was when the member was made, files on its image's disk included,
    and the seed is never handed out."""
    async with client as c:
        httpx_client = c.get_async_httpx_client()
        template = (
            oci_sandbox_template(c, hypervisor, bootable_oci_image_ref)
            if image
            else bootable_sandbox_template(c, hypervisor)
        )
        async with template as template_id:
            pool_url = f"{QARAX_URL}/v1/vm-templates/{template_id}/sandbox-pool"
            sandbox_ids = []

            async def claim():
                created = await call_api(
                    create_sandbox,
                    client=c,
                    body=NewSandbox(
                        name=f"e2e-fork-pool-{uuid.uuid4().hex[:8]}",
                        vm_template_id=template_id,
                        idle_timeout_secs=300,
                    ),
                )
                sandbox_ids.append(created.id)
                sandbox = await call_api(get_sandbox, client=c, sandbox_id=created.id)
                assert sandbox.status == SandboxStatus.READY, sandbox.status
                return created

            async def marker(sandbox_id):
                result = await call_api(
                    exec_sandbox,
                    client=c,
                    sandbox_id=sandbox_id,
                    body=ExecSandboxRequest(
                        # From the member's disk, not the page cache it inherited.
                        command=[
                            "/bin/sh",
                            "-c",
                            "echo 3 > /proc/sys/vm/drop_caches; cat /root/marker 2>/dev/null || echo none",
                        ],
                        timeout_secs=15,
                    ),
                )
                assert result.exit_code == 0, result.stderr
                return result.stdout.strip()

            try:
                response = await httpx_client.put(pool_url, json={"min_ready": 1, "fork": True})
                assert response.status_code == 200, response.text
                assert response.json()["fork"] is True
                pool = await wait_for_sandbox_pool_ready(c, template_id, min_ready=1)
                [seed] = pool["seeds"]
                assert seed["status"] == "ready", pool
                seed_vm_id = uuid.UUID(seed["vm_id"])

                # The member the pool has made doesn't have this; the next does.
                # Synced, so that the member reads it from its image's disk
                # rather than from the page cache it inherits (a kernel
                # template's root is in memory, and its initramfs has no sync).
                write = "echo seeded > /root/marker" + (" && sync" if image else "")
                result = await call_api(
                    exec_vm,
                    client=c,
                    vm_id=seed_vm_id,
                    body=ExecVmRequest(command=["/bin/sh", "-c", write], timeout_secs=15),
                )
                assert result.exit_code == 0, result.stderr

                first = await claim()
                assert first.vm_id != seed_vm_id
                assert await marker(first.id) == "none"

                await wait_for_sandbox_pool_ready(c, template_id, min_ready=1)
                second = await claim()
                assert second.vm_id != seed_vm_id
                assert await marker(second.id) == "seeded"

                seed = await call_api(get_vm, client=c, vm_id=seed_vm_id)
                assert seed.status == VmStatus.RUNNING, seed.status
            finally:
                for sandbox_id in sandbox_ids:
                    await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
                # Deleting the pool destroys its seed and members.
                response = await httpx_client.delete(pool_url)
                assert response.status_code in (204, 404), response.text


@pytest.mark.asyncio
async def test_a_pool_that_keeps_only_its_seed_forks_it_for_each_sandbox(client):
    """A forking pool with min_ready 0 keeps its seed and nothing else: each
    sandbox asked for is forked from the seed on its request, with the seed's
    state as it is then, and the seed keeps running."""
    async with client as c:
        httpx_client = c.get_async_httpx_client()
        async with bootable_sandbox_template(c, "firecracker") as template_id:
            pool_url = f"{QARAX_URL}/v1/vm-templates/{template_id}/sandbox-pool"
            sandbox_ids = []
            try:
                response = await httpx_client.put(pool_url, json={"min_ready": 0, "fork": True})
                assert response.status_code == 200, response.text
                pool = await wait_for_sandbox_pool(
                    c,
                    template_id,
                    lambda pool: [seed["status"] for seed in pool["seeds"]] == ["ready"],
                    "a ready seed",
                )
                seed_vm_id = uuid.UUID(pool["seeds"][0]["vm_id"])

                for marker in ("first", "second"):
                    result = await call_api(
                        exec_vm,
                        client=c,
                        vm_id=seed_vm_id,
                        body=ExecVmRequest(
                            command=["/bin/sh", "-c", f"echo {marker} > /root/marker"],
                            timeout_secs=15,
                        ),
                    )
                    assert result.exit_code == 0, result.stderr
                    created = await call_api(
                        create_sandbox,
                        client=c,
                        body=NewSandbox(
                            name=f"e2e-seed-only-{uuid.uuid4().hex[:8]}",
                            vm_template_id=template_id,
                            idle_timeout_secs=300,
                        ),
                    )
                    sandbox_ids.append(created.id)
                    assert created.vm_id != seed_vm_id
                    sandbox = await call_api(get_sandbox, client=c, sandbox_id=created.id)
                    assert sandbox.status == SandboxStatus.READY, sandbox.status
                    result = await call_api(
                        exec_sandbox,
                        client=c,
                        sandbox_id=created.id,
                        body=ExecSandboxRequest(command=["cat", "/root/marker"], timeout_secs=15),
                    )
                    assert result.exit_code == 0, result.stderr
                    assert result.stdout.strip() == marker

                # It kept nothing ready, and the same seed.
                pool = (await httpx_client.get(pool_url)).json()
                assert pool["current_ready"] == 0, pool
                assert [seed["vm_id"] for seed in pool["seeds"]] == [str(seed_vm_id)], pool
                seed = await call_api(get_vm, client=c, vm_id=seed_vm_id)
                assert seed.status == VmStatus.RUNNING, seed.status
            finally:
                for sandbox_id in sandbox_ids:
                    await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
                # Deleting the pool destroys its seed.
                response = await httpx_client.delete(pool_url)
                assert response.status_code in (204, 404), response.text


@pytest.mark.asyncio
async def test_a_forking_pool_keeps_a_seed_on_each_of_its_seed_hosts(client):
    """A forking pool with seed_hosts 2 keeps a seed on each of two hosts and
    spreads its members across them, each a fork of its own host's seed."""
    async with client as c:
        hosts = up_hosts((await call_api(list_hosts, client=c)).items)
        if len(hosts) < 2:
            pytest.skip("A seed per host needs two UP hosts")
        httpx_client = c.get_async_httpx_client()
        # On shared storage, so that a VM of the template runs on either host.
        async with bootable_sandbox_template(c, "firecracker", shared_pool=True) as template_id:
            pool_url = f"{QARAX_URL}/v1/vm-templates/{template_id}/sandbox-pool"
            sandbox_ids = []

            async def claim():
                created = await call_api(
                    create_sandbox,
                    client=c,
                    body=NewSandbox(
                        name=f"e2e-seed-hosts-{uuid.uuid4().hex[:8]}",
                        vm_template_id=template_id,
                        idle_timeout_secs=300,
                    ),
                )
                sandbox_ids.append(created.id)
                return created

            async def host_of(created):
                return (await call_api(get_vm, client=c, vm_id=created.vm_id)).host_id

            async def marker(created):
                result = await call_api(
                    exec_sandbox,
                    client=c,
                    sandbox_id=created.id,
                    body=ExecSandboxRequest(
                        command=["/bin/sh", "-c", "cat /root/marker 2>/dev/null || echo none"],
                        timeout_secs=15,
                    ),
                )
                assert result.exit_code == 0, result.stderr
                return result.stdout.strip()

            try:
                # The seeds first: a member made while only one runs would
                # go to its host.
                body = {"min_ready": 0, "fork": True, "seed_hosts": 2}
                response = await httpx_client.put(pool_url, json=body)
                assert response.status_code == 200, response.text
                pool = await wait_for_sandbox_pool(
                    c,
                    template_id,
                    lambda pool: [seed["status"] for seed in pool["seeds"]] == ["ready", "ready"],
                    "two ready seeds",
                )
                seeds = {uuid.UUID(seed["host_id"]): uuid.UUID(seed["vm_id"]) for seed in pool["seeds"]}
                assert len(seeds) == 2, f"two seeds on one host: {pool}"

                response = await httpx_client.put(pool_url, json={**body, "min_ready": 2})
                assert response.status_code == 200, response.text
                await wait_for_sandbox_pool_ready(c, template_id, min_ready=2)
                # Each seed marks the members forked from it from now on.
                for host_id, seed_vm_id in seeds.items():
                    result = await call_api(
                        exec_vm,
                        client=c,
                        vm_id=seed_vm_id,
                        body=ExecVmRequest(
                            command=["/bin/sh", "-c", f"echo {host_id} > /root/marker"],
                            timeout_secs=15,
                        ),
                    )
                    assert result.exit_code == 0, result.stderr

                # The two members made before, one on each host; then the two
                # the pool makes up for them, one on each host too, forked
                # from that host's seed.
                before = [await claim(), await claim()]
                assert {await host_of(created) for created in before} == set(seeds)
                await wait_for_sandbox_pool_ready(c, template_id, min_ready=2)
                after = [await claim(), await claim()]
                after_hosts = [await host_of(created) for created in after]
                assert set(after_hosts) == set(seeds), after_hosts
                for created, host_id in zip(after, after_hosts):
                    assert await marker(created) == str(host_id)

                for seed_vm_id in seeds.values():
                    seed = await call_api(get_vm, client=c, vm_id=seed_vm_id)
                    assert seed.status == VmStatus.RUNNING, seed.status
            finally:
                for sandbox_id in sandbox_ids:
                    await delete_sandbox.asyncio_detailed(client=c, sandbox_id=sandbox_id)
                # Deleting the pool destroys its seeds and members.
                response = await httpx_client.delete(pool_url)
                assert response.status_code in (204, 404), response.text


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
