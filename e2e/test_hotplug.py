"""
E2E tests for disk and NIC hotplug / hotunplug.

Covers:
  - Attaching a disk to a VM in Created state (DB-only, no gRPC)
  - Removing a disk from a Created VM
  - Adding / removing a NIC on a Created VM
  - Hotplugging a disk into a running VM (NFS-backed, real Cloud Hypervisor call)
  - Hotunplugging a disk from a running VM
  - Hotplugging a NIC into a running VM (isolated network)
  - Hotunplugging a NIC from a running VM
  - Negative: attach disk to Shutdown VM → 422
  - Negative: remove non-existent disk → 404
  - Negative: duplicate NIC device ID → 409
  - Negative: remove non-existent NIC → 404

Prerequisites for running-VM tests:
  - An NFS server at nfs-server:/nfs-export (shared with live-migration tests)
  - A qarax-node instance reachable with KVM passthrough
"""

import asyncio
import os
import time
import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.networks import (
    attach_host as attach_network_host,
    create as create_network,
    delete as delete_network,
    detach_host as detach_network_host,
)
from qarax_api_client.api.storage_objects import (
    create as create_storage_object,
    delete as delete_storage_object,
)
from qarax_api_client.api.storage_pools import (
    attach_host as attach_pool_host,
    create as create_pool,
    delete as delete_pool,
)
from qarax_api_client.api.transfers import (
    create as create_transfer,
    get as get_transfer,
)
from qarax_api_client.api.vms import (
    add_nic,
    attach_disk,
    create as create_vm,
    delete as delete_vm,
    get as get_vm,
    remove_disk,
    remove_nic,
    start as start_vm,
    stop as stop_vm,
)
from qarax_api_client.models import (
    Hypervisor,
    NewNetwork,
    NewStoragePool,
    NewTransfer,
    NewVm,
    StoragePoolType,
    TransferStatus,
    VmStatus,
)
from qarax_api_client.models.attach_disk_request import AttachDiskRequest
from qarax_api_client.models.attach_host_request import AttachHostRequest
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest
from qarax_api_client.models.new_storage_object import NewStorageObject
from qarax_api_client.models.new_vm_network import NewVmNetwork
from qarax_api_client.models.storage_object_type import StorageObjectType

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
NFS_SERVER_HOST = os.getenv("NFS_SERVER_HOST", "nfs-server")
NFS_EXPORT_PATH = os.getenv("NFS_EXPORT_PATH", "/nfs-export")
VM_OPERATION_TIMEOUT = 30


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


async def call_api(endpoint_module, **kwargs):
    asyncio_fn = getattr(endpoint_module, "asyncio", None)
    if callable(asyncio_fn):
        return await asyncio_fn(**kwargs)
    detailed_fn = getattr(endpoint_module, "asyncio_detailed", None)
    if callable(detailed_fn):
        return (await detailed_fn(**kwargs)).parsed
    raise AttributeError(f"{endpoint_module.__name__} has no async entrypoint")


async def wait_for_status(c, vm_id, expected_status, timeout=VM_OPERATION_TIMEOUT):
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


async def _make_nfs_pool(c, test_id, hosts):
    """Create an NFS storage pool, attach it to the first host, return pool_id."""
    pool_id_raw = await create_pool.asyncio(
        client=c,
        body=NewStoragePool(
            name=f"e2e-hp-pool-{test_id}",
            pool_type=StoragePoolType.NFS,
            config={"url": f"{NFS_SERVER_HOST}:{NFS_EXPORT_PATH}"},
        ),
    )
    pool_id = UUID(str(pool_id_raw).strip('"'))
    await attach_pool_host.asyncio_detailed(
        client=c,
        pool_id=pool_id,
        body=AttachPoolHostRequest(host_id=hosts[0].id),
    )
    return pool_id


async def _make_disk(c, test_id, pool_id):
    """Create a 256 MiB disk storage object on pool_id, return disk_id."""
    disk_id_raw = await create_storage_object.asyncio(
        client=c,
        body=NewStorageObject(
            name=f"e2e-hp-disk-{test_id}",
            storage_pool_id=str(pool_id),
            object_type=StorageObjectType.DISK,
            size_bytes=256 * 1024 * 1024,
        ),
    )
    return UUID(str(disk_id_raw).strip('"'))


async def _make_vm(c, test_id):
    """Create a minimal VM and return its UUID."""
    vm_id_raw = await create_vm.asyncio(
        client=c,
        body=NewVm(
            name=f"e2e-hp-vm-{test_id}",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        ),
    )
    return UUID(str(vm_id_raw).strip('"'))


# ─── Created-state tests (no VM boot required) ────────────────────────────────


@pytest.mark.asyncio
async def test_attach_disk_to_created_vm(client):
    """Attach a disk to a Created VM — recorded in DB only, not hotplugged."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        pool_id = disk_id = vm_id = None
        try:
            pool_id = await _make_nfs_pool(c, test_id, hosts)
            disk_id = await _make_disk(c, test_id, pool_id)
            vm_id = await _make_vm(c, test_id)

            resp = await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            assert resp.status_code == 201, (
                f"Expected 201, got {resp.status_code}: {resp.content.decode()}"
            )
            attached = resp.parsed
            assert attached is not None
            assert str(attached.storage_object_id) == str(disk_id)
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if disk_id:
                await delete_storage_object.asyncio_detailed(client=c, object_id=disk_id)
            if pool_id:
                await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)


@pytest.mark.asyncio
async def test_remove_disk_from_created_vm(client):
    """Remove a disk from a Created VM — DB-only deletion, no gRPC call."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        pool_id = disk_id = vm_id = None
        try:
            pool_id = await _make_nfs_pool(c, test_id, hosts)
            disk_id = await _make_disk(c, test_id, pool_id)
            vm_id = await _make_vm(c, test_id)

            # Attach first
            attach_resp = await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            assert attach_resp.status_code == 201
            device_id = attach_resp.parsed.logical_name

            # Then remove
            rm_resp = await remove_disk.asyncio_detailed(
                client=c, vm_id=vm_id, device_id=device_id
            )
            assert rm_resp.status_code == 204, (
                f"Expected 204, got {rm_resp.status_code}: {rm_resp.content.decode()}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if disk_id:
                await delete_storage_object.asyncio_detailed(client=c, object_id=disk_id)
            if pool_id:
                await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)


@pytest.mark.asyncio
async def test_add_remove_nic_created_vm(client):
    """Add and remove NICs on a Created VM without booting it."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        vm_id = None
        try:
            vm_id = await _make_vm(c, test_id)

            # Add net0
            add_resp = await add_nic.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=NewVmNetwork(id="net0"),
            )
            assert add_resp.status_code == 201, (
                f"Expected 201, got {add_resp.status_code}: {add_resp.content.decode()}"
            )
            assert add_resp.parsed.device_id == "net0"

            # Add net1
            add_resp2 = await add_nic.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=NewVmNetwork(id="net1"),
            )
            assert add_resp2.status_code == 201

            # Remove net0
            rm_resp = await remove_nic.asyncio_detailed(
                client=c, vm_id=vm_id, device_id="net0"
            )
            assert rm_resp.status_code == 204, (
                f"Expected 204, got {rm_resp.status_code}: {rm_resp.content.decode()}"
            )

            # Remove net1
            await remove_nic.asyncio_detailed(client=c, vm_id=vm_id, device_id="net1")
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


# ─── Negative tests ───────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_attach_disk_shutdown_vm_returns_422(client):
    """Attaching a disk to a Shutdown VM returns 422."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = await list_hosts.asyncio(client=c)
        assert hosts, "No hosts available"
        pool_id = disk_id = vm_id = None
        try:
            pool_id = await _make_nfs_pool(c, test_id, hosts)
            disk_id = await _make_disk(c, test_id, pool_id)
            vm_id = await _make_vm(c, test_id)

            # Start then stop so status = SHUTDOWN
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)
            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)

            resp = await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            assert resp.status_code == 422, (
                f"Expected 422 for SHUTDOWN VM, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if disk_id:
                await delete_storage_object.asyncio_detailed(client=c, object_id=disk_id)
            if pool_id:
                await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)


@pytest.mark.asyncio
async def test_remove_disk_not_found_returns_404(client):
    """Removing a non-existent disk returns 404."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        vm_id = None
        try:
            vm_id = await _make_vm(c, test_id)
            resp = await remove_disk.asyncio_detailed(
                client=c, vm_id=vm_id, device_id="nonexistent"
            )
            assert resp.status_code == 404, (
                f"Expected 404, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_add_nic_duplicate_device_id_returns_409(client):
    """Adding a NIC with a duplicate device ID returns 409."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        vm_id = None
        try:
            vm_id = await _make_vm(c, test_id)

            resp1 = await add_nic.asyncio_detailed(
                client=c, vm_id=vm_id, body=NewVmNetwork(id="net0")
            )
            assert resp1.status_code == 201

            resp2 = await add_nic.asyncio_detailed(
                client=c, vm_id=vm_id, body=NewVmNetwork(id="net0")
            )
            assert resp2.status_code == 409, (
                f"Expected 409 for duplicate device ID, got {resp2.status_code}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_remove_nic_not_found_returns_404(client):
    """Removing a non-existent NIC returns 404."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        vm_id = None
        try:
            vm_id = await _make_vm(c, test_id)
            resp = await remove_nic.asyncio_detailed(
                client=c, vm_id=vm_id, device_id="net99"
            )
            assert resp.status_code == 404, (
                f"Expected 404, got {resp.status_code}"
            )
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


# ─── Running-VM hotplug tests (require NFS + real Cloud Hypervisor) ───────────


@pytest.mark.asyncio
async def test_disk_hotplug_running_vm(client):
    """
    Hotplug a disk into a running VM, verify the VM stays Running, then hotunplug.

    Uses an NFS storage pool and a transfer to provision an actual disk file before
    hotplugging — Cloud Hypervisor requires the file to exist at hotplug time.
    The pool is attached to all hosts so hotplug works regardless of VM placement.
    """
    test_id = uuid.uuid4().hex[:8]
    pool_id = disk_id = vm_id = None
    async with client as c:
        try:
            hosts = await list_hosts.asyncio(client=c)
            assert hosts, "No hosts available"

            # Create an NFS pool and attach it to all hosts so the disk file is accessible
            # regardless of which host the VM is scheduled on.
            pool_id = await _make_nfs_pool(c, test_id, hosts)
            for host in hosts[1:]:
                await attach_pool_host.asyncio_detailed(
                    client=c,
                    pool_id=pool_id,
                    body=AttachPoolHostRequest(host_id=host.id),
                )

            # Provision the disk file via a transfer so the file actually exists on the node.
            # Cloud Hypervisor requires the file to be present at hotplug time.
            # We copy the test kernel as a raw "disk" image — CH accepts any file as raw block.
            transfer = await create_transfer.asyncio(
                client=c,
                pool_id=str(pool_id),
                body=NewTransfer(
                    name=f"e2e-hp-disk-{test_id}",
                    source="/var/lib/qarax/images/vmlinux",
                    object_type=StorageObjectType.DISK,
                ),
            )
            assert transfer is not None, "Transfer creation failed"

            # Wait for the transfer to complete and get the resulting storage object ID.
            start = time.time()
            while time.time() - start < 30:
                transfer = await get_transfer.asyncio(
                    client=c,
                    pool_id=str(pool_id),
                    transfer_id=str(transfer.id),
                )
                if transfer.status == TransferStatus.COMPLETED:
                    break
                if transfer.status == TransferStatus.FAILED:
                    pytest.fail(f"Transfer failed: {transfer.error_message}")
                await asyncio.sleep(0.5)
            else:
                pytest.fail(f"Transfer did not complete within 30s (status: {transfer.status})")

            disk_id = transfer.storage_object_id
            assert disk_id is not None, "Transfer completed but no storage object ID"

            vm_id = await _make_vm(c, test_id)

            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            # Hotplug — VM is Running so gRPC add_disk_device is called
            attach_resp = await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            assert attach_resp.status_code == 201, (
                f"Disk hotplug failed: {attach_resp.status_code} {attach_resp.content.decode()}"
            )
            device_id = attach_resp.parsed.logical_name

            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, "VM should still be running after disk hotplug"

            # Hotunplug
            rm_resp = await remove_disk.asyncio_detailed(
                client=c, vm_id=vm_id, device_id=device_id
            )
            assert rm_resp.status_code == 204, (
                f"Disk hotunplug failed: {rm_resp.status_code} {rm_resp.content.decode()}"
            )

            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, (
                "VM should still be running after disk hotunplug"
            )

            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if disk_id:
                try:
                    await delete_storage_object.asyncio_detailed(client=c, object_id=disk_id)
                except Exception:
                    pass
            if pool_id:
                try:
                    await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)
                except Exception:
                    pass


@pytest.mark.asyncio
async def test_nic_hotplug_running_vm(client):
    """
    Hotplug a NIC into a running VM, verify IP allocation and VM stays Running,
    then hotunplug.
    """
    test_id = uuid.uuid4().hex[:8]
    net_id_str = host_id_str = vm_id = None
    async with client as c:
        try:
            hosts = await list_hosts.asyncio(client=c)
            assert hosts, "No hosts available"
            host_id_str = str(hosts[0].id)

            # Create an isolated network for the hotplugged NIC
            net_id_raw = await create_network.asyncio(
                client=c,
                body=NewNetwork(
                    name=f"e2e-hp-net-{test_id}",
                    subnet="10.97.0.0/24",
                    gateway="10.97.0.1",
                    type_="isolated",
                ),
            )
            net_id = UUID(str(net_id_raw).strip('"'))
            net_id_str = str(net_id)

            attach_net_resp = await attach_network_host.asyncio_detailed(
                client=c,
                network_id=net_id_str,
                body=AttachHostRequest(
                    host_id=hosts[0].id,
                    bridge_name=f"br-10-97-{test_id[:4]}",
                ),
            )
            assert attach_net_resp.status_code in (200, 204), (
                f"Network attach failed: {attach_net_resp.status_code}"
            )

            vm_id = await _make_vm(c, test_id)
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)

            # Hotplug a NIC onto the isolated network
            add_resp = await add_nic.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=NewVmNetwork(id="net0", network_id=net_id),
            )
            assert add_resp.status_code == 201, (
                f"NIC hotplug failed: {add_resp.status_code} {add_resp.content.decode()}"
            )
            nic = add_resp.parsed
            assert nic.device_id == "net0"
            assert nic.ip_address is not None, "Expected an IP to be allocated"

            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, "VM should still be running after NIC hotplug"

            # Hotunplug
            rm_resp = await remove_nic.asyncio_detailed(
                client=c, vm_id=vm_id, device_id="net0"
            )
            assert rm_resp.status_code == 204, (
                f"NIC hotunplug failed: {rm_resp.status_code} {rm_resp.content.decode()}"
            )

            vm = await get_vm.asyncio(client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, (
                "VM should still be running after NIC hotunplug"
            )

            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
        finally:
            if vm_id:
                await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)
            if net_id_str and host_id_str:
                try:
                    await detach_network_host.asyncio_detailed(
                        client=c, network_id=net_id_str, host_id=host_id_str
                    )
                except Exception:
                    pass
            if net_id_str:
                try:
                    await delete_network.asyncio_detailed(client=c, network_id=net_id_str)
                except Exception:
                    pass
