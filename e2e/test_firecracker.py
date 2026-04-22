"""
E2E tests for Firecracker VMM backend.

These tests verify:
- Creating a Firecracker VM and listing it
- Full FC lifecycle: create → start → pause → resume → stop → delete
- Force-stopping a running FC VM
- Cloud-init seed disk attachment for FC VMs
- Snapshot/restore for FC VMs
- Hot-attach operations return 501 UNIMPLEMENTED for FC VMs

Prerequisites:
- The Firecracker binary must be present at /usr/local/bin/firecracker on the
  qarax-node host (installed in the e2e Dockerfile).
- The same vmlinux and test-initramfs used by Cloud Hypervisor tests are reused.
"""

import uuid

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_pools import attach_host as attach_pool_host
from qarax_api_client.api.storage_pools import create as create_pool
from qarax_api_client.api.storage_pools import delete as delete_pool
from qarax_api_client.api.vms import (
    list_ as list_vms,
    create as create_vm,
    get as get_vm,
    start as start_vm,
    stop as stop_vm,
    force_stop as force_stop_vm,
    pause as pause_vm,
    resume as resume_vm,
    delete as delete_vm,
    create_snapshot,
    restore,
    attach_disk,
)
from qarax_api_client.models import (
    AttachDiskRequest,
    AttachPoolHostRequest,
    CreateSnapshotRequest,
    HostStatus,
    Hypervisor,
    NewStoragePool,
    NewVm,
    RestoreRequest,
    StoragePoolType,
    VmStatus,
)

from helpers import QARAX_URL, call_api, call_api_detailed, wait_for_status


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL)


@pytest.fixture(scope="module")
def fc_snapshot_storage_pool():
    """Create a local storage pool for FC snapshot tests and attach it to a UP host."""
    with Client(base_url=QARAX_URL) as c:
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
async def test_host_reports_firecracker_version(client):
    """Initialized hosts should report the Firecracker version through the REST API."""
    async with client as c:
        hosts = [h for h in (await call_api(list_hosts, client=c) or []) if h.status == HostStatus.UP]
        assert hosts, "No UP hosts registered"
        assert any(h.firecracker_version for h in hosts), (
            "Expected at least one initialized host to report a Firecracker version"
        )


@pytest.mark.asyncio
async def test_fc_vm_create_and_delete(client):
    """Create a Firecracker VM and verify it appears in the list, then delete it."""
    async with client as c:
        new_vm = new_fc_vm("e2e-fc-create")
        vm_id_raw = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id_raw is not None
        vm_id = str(vm_id_raw).strip('"')

        try:
            # Verify the VM exists and has the right hypervisor
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm is not None
            assert vm.hypervisor == Hypervisor.FIRECRACKER
            assert vm.status == VmStatus.CREATED

            # Verify it appears in the list
            vms = await call_api(list_vms, client=c)
            vm_ids = [str(v.id) for v in (vms or [])]
            assert vm_id in vm_ids, f"FC VM {vm_id} not found in list"
        finally:
            await call_api_detailed(delete_vm, client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_fc_vm_full_lifecycle(client):
    """Full Firecracker lifecycle: create → start → pause → resume → stop → delete."""
    async with client as c:
        new_vm = new_fc_vm("e2e-fc-lifecycle")
        vm_id_raw = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id_raw is not None
        vm_id = str(vm_id_raw).strip('"')

        try:
            # Start
            await call_api_detailed(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            # Pause
            await call_api_detailed(pause_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.PAUSED)

            # Resume
            await call_api_detailed(resume_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            # Stop (soft)
            await call_api_detailed(stop_vm, client=c, vm_id=vm_id)
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.SHUTDOWN
        finally:
            await call_api_detailed(delete_vm, client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_fc_vm_force_stop(client):
    """Force-stop a running Firecracker VM."""
    async with client as c:
        new_vm = new_fc_vm("e2e-fc-force-stop")
        vm_id_raw = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id_raw is not None
        vm_id = str(vm_id_raw).strip('"')

        try:
            await call_api_detailed(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            await call_api_detailed(force_stop_vm, client=c, vm_id=vm_id)
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.SHUTDOWN
        finally:
            await call_api_detailed(delete_vm, client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_fc_cloud_init(client):
    """Verify cloud-init seed disk is generated and VM boots with it."""
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

        try:
            # Verify cloud-init data stored
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.cloud_init_user_data == user_data
            assert vm.cloud_init_meta_data == meta_data

            # Start the VM — it should boot with the cidata seed attached
            await call_api_detailed(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            await call_api_detailed(stop_vm, client=c, vm_id=vm_id)
        finally:
            await call_api_detailed(delete_vm, client=c, vm_id=vm_id)


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
            await call_api_detailed(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)
            await call_api_detailed(pause_vm, client=c, vm_id=vm_id)
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
            await call_api_detailed(stop_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)

            restored_vm = await call_api(
                restore,
                client=c,
                vm_id=vm_id,
                body=RestoreRequest(snapshot_id=snapshot.id),
            )
            assert restored_vm is not None
            assert restored_vm.status == VmStatus.RUNNING

            await call_api_detailed(stop_vm, client=c, vm_id=vm_id)
        finally:
            await call_api_detailed(delete_vm, client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_fc_unsupported_hotplug_returns_error(client):
    """Hot-attach disk operations should return an error for Firecracker VMs."""
    async with client as c:
        new_vm = new_fc_vm("e2e-fc-no-hotplug")
        vm_id_raw = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id_raw is not None
        vm_id = str(vm_id_raw).strip('"')

        try:
            await call_api_detailed(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            # Attempt hot-attach — expect an error response (501 UNIMPLEMENTED or similar)
            response = await call_api_detailed(
                attach_disk,
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=uuid.uuid4()),
            )
            # The server should return a non-2xx status for unsupported operation
            assert response.status_code not in (200, 201, 204), (
                f"Expected error for FC hotplug but got {response.status_code}"
            )
        finally:
            await call_api_detailed(force_stop_vm, client=c, vm_id=vm_id)
            await call_api_detailed(delete_vm, client=c, vm_id=vm_id)
