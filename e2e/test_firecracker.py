"""
E2E tests for Firecracker VMM backend.

These tests verify:
- Hosts report their Firecracker and node versions
- Full FC lifecycle: create → start → pause → resume → stop → delete
- Force-stopping a running FC VM
- Cloud-init seed disk generation and cleanup for FC VMs
- Snapshot/restore for FC VMs
- Hot-attaching a disk to a running FC VM fails without leaving a disk behind

Prerequisites:
- The Firecracker binary must be present at /usr/local/bin/firecracker on the
  qarax-node host (installed in the e2e Dockerfile).
- The same vmlinux and test-initramfs used by Cloud Hypervisor tests are reused.
"""

import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_objects import delete as delete_storage_object
from qarax_api_client.api.storage_pools import attach_host as attach_pool_host
from qarax_api_client.api.storage_pools import create as create_pool
from qarax_api_client.api.storage_pools import create_disk
from qarax_api_client.api.storage_pools import delete as delete_pool
from qarax_api_client.api.vms import (
    attach_disk,
    create_snapshot,
    remove_disk,
    restore,
)
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import force_stop as force_stop_vm
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.api.vms import pause as pause_vm
from qarax_api_client.api.vms import resume as resume_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.api.vms import stop as stop_vm
from qarax_api_client.models import (
    AttachDiskRequest,
    AttachPoolHostRequest,
    CreateDiskRequest,
    CreateSnapshotRequest,
    HostStatus,
    Hypervisor,
    NewStoragePool,
    NewVm,
    RestoreRequest,
    StoragePoolType,
    VmStatus,
)

from helpers import (
    AUTH_HEADERS,
    NODE_CONTAINER_PREFIX,
    NODE_RUNTIME_DIR,
    QARAX_URL,
    call_api,
    call_api_detailed,
    path_exists_in_any,
    wait_for_status,
)


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


@pytest.fixture(scope="module")
def fc_snapshot_storage_pool():
    """Create a local storage pool for FC snapshot tests and attach it to a UP host."""
    with Client(base_url=QARAX_URL, headers=AUTH_HEADERS) as c:
        hosts = [h for h in (list_hosts.sync(client=c) or []) if h.status == HostStatus.UP]
        assert hosts, "No UP hosts registered"
        host_id = hosts[0].id

        pool_id_raw = create_pool.sync(
            client=c,
            body=NewStoragePool(
                name=f"e2e-fc-snapshot-pool-{uuid.uuid4().hex[:8]}",
                pool_type=StoragePoolType.LOCAL,
                config={"path": "/var/lib/qarax/snapshots"},
            ),
        )
        assert pool_id_raw is not None, "Failed to create FC snapshot storage pool"
        pool_id = uuid.UUID(str(pool_id_raw).strip('"'))

        attach_pool_host.sync_detailed(
            client=c,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=host_id),
        )

        yield pool_id

        delete_pool.sync_detailed(client=c, pool_id=pool_id)


def new_fc_vm(name: str, **kwargs) -> NewVm:
    """Helper: create a NewVm with Firecracker defaults (128 MiB memory)."""
    return NewVm(
        name=name,
        hypervisor=Hypervisor.FIRECRACKER,
        boot_vcpus=1,
        max_vcpus=1,
        memory_size=128 * 1024 * 1024,  # 128 MiB — FC is lightweight
        **kwargs,
    )


@pytest.mark.asyncio
async def test_host_reports_firecracker_version(client, two_hosts):
    """Initialized e2e hosts report their Firecracker and node versions through the REST API."""
    async with client as c:
        hosts = {h.id: h for h in await call_api(list_hosts, client=c)}
        for host_id in two_hosts:
            host = hosts[host_id]
            assert host.firecracker_version, (
                f"Host {host.name} should report a Firecracker version"
            )
            assert host.node_version, (
                f"Host {host.name} should report node_version after init"
            )
            # The e2e nodes are built from the same tree as the control plane.
            assert host.update_available is False, (
                f"Host {host.name} runs node {host.node_version}, which the control "
                "plane considers out of date"
            )


@pytest.mark.asyncio
async def test_fc_vm_full_lifecycle(client):
    """Full Firecracker lifecycle: create → start → pause → resume → stop → start → delete.

    A microVM can't boot twice, so the second start runs on a fresh Firecracker process.
    """
    async with client as c:
        new_vm = new_fc_vm("e2e-fc-lifecycle")
        vm_id_raw = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id_raw is not None
        vm_id = str(vm_id_raw).strip('"')

        try:
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.hypervisor == Hypervisor.FIRECRACKER
            assert vm.status == VmStatus.CREATED

            # Start
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            # Pause
            resp = await call_api_detailed(pause_vm, client=c, vm_id=vm_id)
            assert resp.status_code == 200, f"pause: HTTP {resp.status_code} {resp.content}"
            await wait_for_status(c, vm_id, VmStatus.PAUSED)

            # Resume
            resp = await call_api_detailed(resume_vm, client=c, vm_id=vm_id)
            assert resp.status_code == 200, f"resume: HTTP {resp.status_code} {resp.content}"
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            # Stop (soft)
            await call_api(stop_vm, client=c, vm_id=vm_id)
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.SHUTDOWN

            # Start again
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)
        finally:
            await call_api(delete_vm, client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_fc_vm_force_stop(client):
    """Force-stop a running Firecracker VM, then start it again."""
    async with client as c:
        new_vm = new_fc_vm("e2e-fc-force-stop")
        vm_id_raw = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id_raw is not None
        vm_id = str(vm_id_raw).strip('"')

        try:
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            await call_api(force_stop_vm, client=c, vm_id=vm_id)
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.SHUTDOWN

            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)
        finally:
            await call_api(delete_vm, client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_fc_cloud_init(client):
    """The node writes a cloud-init seed image for the FC VM and removes it with the VM."""
    user_data = """\
#cloud-config
runcmd:
  - echo "fc cloud-init e2e test" > /tmp/fc-cloud-init-ran
"""
    meta_data = "instance-id: e2e-fc-cloud-init\nlocal-hostname: e2e-fc-vm\n"

    async with client as c:
        new_vm = new_fc_vm(
            "e2e-fc-cloud-init",
            cloud_init_user_data=user_data,
            cloud_init_meta_data=meta_data,
        )
        vm_id_raw = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id_raw is not None
        vm_id = str(vm_id_raw).strip('"')
        # qarax-node/src/firecracker/helpers.rs: cloud_init_seed_path
        seed_path = f"{NODE_RUNTIME_DIR}/{vm_id}-fc-cidata.img"

        try:
            # Start the VM — it should boot with the cidata seed attached
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)
            assert path_exists_in_any(NODE_CONTAINER_PREFIX, seed_path), (
                f"cloud-init seed image {seed_path} not found on any node"
            )
        finally:
            await call_api(delete_vm, client=c, vm_id=vm_id)

        assert not path_exists_in_any(NODE_CONTAINER_PREFIX, seed_path), (
            f"cloud-init seed image {seed_path} should be removed with the VM"
        )


@pytest.mark.asyncio
async def test_fc_vm_snapshot_restore(client, fc_snapshot_storage_pool):
    """Snapshot a paused FC VM and restore it."""
    async with client as c:
        new_vm = new_fc_vm("e2e-fc-snapshot")
        vm_id_raw = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id_raw is not None
        vm_id = str(vm_id_raw).strip('"')

        try:
            # Start and pause (FC requires paused state for snapshot)
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)
            resp = await call_api_detailed(pause_vm, client=c, vm_id=vm_id)
            assert resp.status_code == 200, f"pause: HTTP {resp.status_code} {resp.content}"
            await wait_for_status(c, vm_id, VmStatus.PAUSED)

            # Snapshot
            snapshot = await call_api(
                create_snapshot,
                client=c,
                vm_id=vm_id,
                body=CreateSnapshotRequest(),
            )
            assert snapshot is not None

            # Stop, then restore from created snapshot
            await call_api(stop_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)

            restored_vm = await call_api(
                restore,
                client=c,
                vm_id=vm_id,
                body=RestoreRequest(snapshot_id=snapshot.id),
            )
            assert restored_vm is not None
            assert restored_vm.status == VmStatus.RUNNING
        finally:
            await call_api(delete_vm, client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_fc_unsupported_hotplug_returns_error(client):
    """Hot-attaching a real disk to a running FC VM fails and leaves no disk behind."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        vm_id_raw = await call_api(
            create_vm, client=c, body=new_fc_vm(f"e2e-fc-no-hotplug-{test_id}")
        )
        vm_id = str(vm_id_raw).strip('"')
        pool_id = disk_id = None

        try:
            await call_api(start_vm, client=c, vm_id=vm_id)
            vm = await wait_for_status(c, vm_id, VmStatus.RUNNING)

            # A real disk in a local pool on the VM's host, so the request
            # passes the control plane's checks and reaches the node.
            pool_id_raw = await call_api(
                create_pool,
                client=c,
                body=NewStoragePool(
                    name=f"e2e-fc-hotplug-pool-{test_id}",
                    pool_type=StoragePoolType.LOCAL,
                    config={"path": f"/var/lib/qarax/e2e-fc-hotplug-{test_id}"},
                ),
            )
            pool_id = UUID(str(pool_id_raw).strip('"'))
            await call_api(
                attach_pool_host,
                client=c,
                pool_id=pool_id,
                body=AttachPoolHostRequest(host_id=vm.host_id),
            )
            disk = await call_api(
                create_disk,
                client=c,
                pool_id=pool_id,
                body=CreateDiskRequest(
                    name=f"e2e-fc-hotplug-disk-{test_id}", size_bytes=1024 * 1024
                ),
            )
            disk_id = disk.storage_object_id

            response = await call_api_detailed(
                attach_disk,
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            assert response.status_code >= 400, (
                f"Expected FC disk hotplug to fail, got {response.status_code}: "
                f"{response.content}"
            )

            # The failed hotplug must not leave a disk record on the VM.
            removed = await call_api_detailed(
                remove_disk, client=c, vm_id=vm_id, device_id="disk0"
            )
            assert removed.status_code == 404, (
                f"Expected no disk0 on the VM after the failed hotplug, got "
                f"{removed.status_code}: {removed.content}"
            )
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING
        finally:
            await call_api(delete_vm, client=c, vm_id=vm_id)
            if disk_id is not None:
                await call_api(delete_storage_object, client=c, object_id=disk_id)
            if pool_id is not None:
                await call_api(delete_pool, client=c, pool_id=pool_id)
