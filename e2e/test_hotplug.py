"""
E2E tests for disk and NIC hotplug / hotunplug against real Cloud Hypervisor VMs.

Covers:
  - Attaching / removing a disk on a Shutdown VM (applied to CH via gRPC)
  - Hotplugging a disk into a running VM, then hotunplugging it
  - Hotplugging a NIC into a running VM (isolated network), then hotunplugging it

Each disk test uses its own sparse file, created on every node, as the raw
block device.
"""

import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.networks import (
    attach_host as attach_network_host,
)
from qarax_api_client.api.networks import (
    create as create_network,
)
from qarax_api_client.api.networks import (
    delete as delete_network,
)
from qarax_api_client.api.networks import (
    detach_host as detach_network_host,
)
from qarax_api_client.api.storage_objects import (
    create as create_storage_object,
)
from qarax_api_client.api.storage_objects import (
    delete as delete_storage_object,
)
from qarax_api_client.api.storage_pools import (
    attach_host as attach_pool_host,
)
from qarax_api_client.api.storage_pools import (
    create as create_pool,
)
from qarax_api_client.api.storage_pools import (
    delete as delete_pool,
)
from qarax_api_client.api.vms import (
    add_nic,
    attach_disk,
    remove_disk,
    remove_nic,
)
from qarax_api_client.api.vms import (
    create as create_vm,
)
from qarax_api_client.api.vms import (
    delete as delete_vm,
)
from qarax_api_client.api.vms import (
    get as get_vm,
)
from qarax_api_client.api.vms import (
    start as start_vm,
)
from qarax_api_client.api.vms import (
    stop as stop_vm,
)
from qarax_api_client.models import (
    Hypervisor,
    NewNetwork,
    NewStoragePool,
    NewVm,
    StoragePoolType,
    VmStatus,
)
from qarax_api_client.models.attach_disk_request import AttachDiskRequest
from qarax_api_client.models.attach_host_request import AttachHostRequest
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest
from qarax_api_client.models.new_storage_object import NewStorageObject
from qarax_api_client.models.new_vm_network import NewVmNetwork
from qarax_api_client.models.storage_object_type import StorageObjectType

from helpers import (
    AUTH_HEADERS,
    QARAX_URL,
    call_api,
    create_sparse_file_on_nodes,
    remove_file_from_nodes,
    wait_for_status,
)
from helpers import up_hosts as _up_hosts

DISK_SIZE = 64 * 1024 * 1024


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


async def _make_vm(c, test_id):
    """Create a minimal VM and return its UUID."""
    vm_id_raw = await call_api(
        create_vm,
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


async def _make_local_disk(c, test_id, hosts, prefix):
    """Create a local pool on every UP host and a disk object backed by a per-test sparse file.

    attach_disk requires a Running/Shutdown VM's host to have the local pool,
    so the pool goes on every host to stay independent of VM placement.
    Returns (pool_id, disk_id, disk_path).
    """
    pool_dir = f"/var/lib/qarax/e2e-{prefix}-{test_id}"
    pool_id_raw = await call_api(
        create_pool,
        client=c,
        body=NewStoragePool(
            name=f"e2e-{prefix}-pool-{test_id}",
            pool_type=StoragePoolType.LOCAL,
            config={"path": pool_dir},
        ),
    )
    pool_id = UUID(str(pool_id_raw).strip('"'))
    for host in _up_hosts(hosts):
        await call_api(
            attach_pool_host,
            client=c,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=host.id),
        )

    disk_path = f"{pool_dir}/hotplug-disk.raw"
    create_sparse_file_on_nodes(disk_path, DISK_SIZE)
    disk_id_raw = await call_api(
        create_storage_object,
        client=c,
        body=NewStorageObject(
            name=f"e2e-{prefix}-disk-{test_id}",
            storage_pool_id=str(pool_id),
            object_type=StorageObjectType.DISK,
            size_bytes=DISK_SIZE,
            config={"path": disk_path},
        ),
    )
    return pool_id, UUID(str(disk_id_raw).strip('"')), disk_path


@pytest.mark.asyncio
async def test_attach_remove_disk_shutdown_vm(client):
    """Attach and remove a disk on a Shutdown VM — applied to CH immediately."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        hosts = _up_hosts(await call_api(list_hosts, client=c))
        assert hosts, "No UP hosts available"
        pool_id = disk_id = vm_id = disk_path = None
        try:
            pool_id, disk_id, disk_path = await _make_local_disk(c, test_id, hosts, "hp-sd")
            vm_id = await _make_vm(c, test_id)

            # Start then stop so status = SHUTDOWN
            await call_api(start_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING)
            await call_api(stop_vm, client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)

            # Attach disk to shutdown VM — should succeed and apply via gRPC
            resp = await attach_disk.asyncio_detailed(
                client=c,
                vm_id=vm_id,
                body=AttachDiskRequest(storage_object_id=disk_id),
            )
            assert resp.status_code == 201, (
                f"Expected 201 for SHUTDOWN VM, got {resp.status_code}: {resp.content.decode()}"
            )
            device_id = resp.parsed.logical_name

            # Verify VM is still shutdown
            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.SHUTDOWN

            # Remove the disk while still shutdown
            rm_resp = await remove_disk.asyncio_detailed(
                client=c, vm_id=vm_id, device_id=device_id
            )
            assert rm_resp.status_code == 204, (
                f"Expected 204 for disk removal on SHUTDOWN VM, got {rm_resp.status_code}"
            )
        finally:
            if vm_id:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if disk_id:
                await call_api(delete_storage_object, client=c, object_id=disk_id)
            if pool_id:
                await call_api(delete_pool, client=c, pool_id=pool_id)
            if disk_path:
                remove_file_from_nodes(disk_path)


# Running-VM hotplug tests (require real Cloud Hypervisor)


@pytest.mark.asyncio
async def test_disk_hotplug_running_vm(client):
    """
    Hotplug a disk into a running VM, verify the VM stays Running, then hotunplug.

    Uses a local pool attached to all hosts so the hotplug works regardless of VM
    placement, and a per-test sparse file that Cloud Hypervisor opens as a raw
    block device.
    """
    test_id = uuid.uuid4().hex[:8]
    pool_id = disk_id = vm_id = disk_path = None
    async with client as c:
        try:
            hosts = _up_hosts(await call_api(list_hosts, client=c))
            assert hosts, "No UP hosts available"
            pool_id, disk_id, disk_path = await _make_local_disk(c, test_id, hosts, "hp")

            vm_id = await _make_vm(c, test_id)

            await call_api(start_vm, client=c, vm_id=vm_id)
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

            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, (
                "VM should still be running after disk hotplug"
            )

            # Hotunplug
            rm_resp = await remove_disk.asyncio_detailed(
                client=c, vm_id=vm_id, device_id=device_id
            )
            assert rm_resp.status_code == 204, (
                f"Disk hotunplug failed: {rm_resp.status_code} {rm_resp.content.decode()}"
            )

            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, (
                "VM should still be running after disk hotunplug"
            )
        finally:
            if vm_id:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            if disk_id:
                await call_api(delete_storage_object, client=c, object_id=disk_id)
            if pool_id:
                await call_api(delete_pool, client=c, pool_id=pool_id)
            if disk_path:
                remove_file_from_nodes(disk_path)


@pytest.mark.asyncio
async def test_nic_hotplug_running_vm(client):
    """
    Hotplug a NIC into a running VM, verify IP allocation and VM stays Running,
    then hotunplug.
    """
    test_id = uuid.uuid4().hex[:8]
    net_id = vm_id = None
    attached_host_ids = []
    async with client as c:
        try:
            hosts = _up_hosts(await call_api(list_hosts, client=c))
            assert hosts, "No UP hosts available"

            # Create an isolated network for the hotplugged NIC
            net_id_raw = await call_api(
                create_network,
                client=c,
                body=NewNetwork(
                    name=f"e2e-hp-net-{test_id}",
                    subnet="10.97.0.0/24",
                    gateway="10.97.0.1",
                    type_="isolated",
                ),
            )
            net_id = UUID(str(net_id_raw).strip('"'))

            # The VM may land on any UP host, so the network must exist on each.
            for host in hosts:
                await call_api(
                    attach_network_host,
                    client=c,
                    network_id=net_id,
                    body=AttachHostRequest(
                        host_id=host.id,
                        bridge_name=f"br-10-97-{test_id[:4]}",
                    ),
                )
                attached_host_ids.append(host.id)

            vm_id = await _make_vm(c, test_id)
            await call_api(start_vm, client=c, vm_id=vm_id)
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

            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, (
                "VM should still be running after NIC hotplug"
            )

            # Hotunplug
            rm_resp = await remove_nic.asyncio_detailed(
                client=c, vm_id=vm_id, device_id="net0"
            )
            assert rm_resp.status_code == 204, (
                f"NIC hotunplug failed: {rm_resp.status_code} {rm_resp.content.decode()}"
            )

            vm = await call_api(get_vm, client=c, vm_id=vm_id)
            assert vm.status == VmStatus.RUNNING, (
                "VM should still be running after NIC hotunplug"
            )
        finally:
            if vm_id:
                await call_api(delete_vm, client=c, vm_id=vm_id)
            for host_id in attached_host_ids:
                await call_api(
                    detach_network_host, client=c, network_id=net_id, host_id=host_id
                )
            if net_id:
                await call_api(delete_network, client=c, network_id=net_id)
