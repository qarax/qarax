"""
E2E tests for VM boot_mode and disk attachment with logical_name.

These tests verify:
- Creating VMs with default and explicit boot_mode values
- Fetching VMs returns the correct boot_mode
- Attaching disks with auto-generated and explicit logical_name
- Auto-incrementing logical_name across multiple disk attachments
"""

import os

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
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
    attach_disk as attach_disk_api,
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
    list_ as list_vms,
)
from qarax_api_client.models import (
    AttachDiskRequest,
    BootMode,
    Hypervisor,
    NewStoragePool,
    NewVm,
    StoragePoolType,
)
from qarax_api_client.models.attach_host_request import AttachHostRequest
from qarax_api_client.models.new_storage_object import NewStorageObject
from qarax_api_client.models.storage_object_type import StorageObjectType

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


# boot_mode tests


@pytest.mark.asyncio
async def test_create_vm_default_boot_mode(client):
    """VM created without boot_mode should default to 'kernel'."""
    async with client as c:
        new_vm = NewVm(
            name="e2e-vm-default-boot",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=268435456,
            config={},
        )
        result = await create_vm.asyncio(client=c, body=new_vm)
        assert result is not None
        vm_id = result if isinstance(result, str) else result.vm_id
        vm_id_str = str(vm_id).strip('"')

        try:
            from uuid import UUID

            vm = await get_vm.asyncio(client=c, vm_id=UUID(vm_id_str))
            assert vm is not None
            assert vm.boot_mode == BootMode.KERNEL
        finally:
            from uuid import UUID

            await delete_vm.asyncio_detailed(client=c, vm_id=UUID(vm_id_str))


@pytest.mark.asyncio
async def test_create_vm_firmware_boot_mode(client):
    """VM created with boot_mode='firmware' should retain that value."""
    async with client as c:
        new_vm = NewVm(
            name="e2e-vm-firmware-boot",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=268435456,
            boot_mode=BootMode.FIRMWARE,
            config={},
        )
        result = await create_vm.asyncio(client=c, body=new_vm)
        assert result is not None
        vm_id = result if isinstance(result, str) else result.vm_id
        vm_id_str = str(vm_id).strip('"')

        try:
            from uuid import UUID

            vm = await get_vm.asyncio(client=c, vm_id=UUID(vm_id_str))
            assert vm is not None
            assert vm.boot_mode == BootMode.FIRMWARE
        finally:
            from uuid import UUID

            await delete_vm.asyncio_detailed(client=c, vm_id=UUID(vm_id_str))


@pytest.mark.asyncio
async def test_list_vms_boot_mode(client):
    """Listed VMs should include boot_mode field."""
    async with client as c:
        # Create two VMs with different boot modes
        vm_ids = []
        for name, mode in [
            ("e2e-list-kernel", None),
            ("e2e-list-firmware", BootMode.FIRMWARE),
        ]:
            new_vm = NewVm(
                name=name,
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=268435456,
                boot_mode=mode,
                config={},
            )
            result = await create_vm.asyncio(client=c, body=new_vm)
            assert result is not None
            vm_id = result if isinstance(result, str) else result.vm_id
            vm_ids.append(str(vm_id).strip('"'))

        try:
            vms = await list_vms.asyncio(client=c)
            assert vms is not None

            kernel_vm = next((v for v in vms if v.name == "e2e-list-kernel"), None)
            firmware_vm = next((v for v in vms if v.name == "e2e-list-firmware"), None)
            assert kernel_vm is not None, "Kernel VM not found in list"
            assert firmware_vm is not None, "Firmware VM not found in list"
            assert kernel_vm.boot_mode == BootMode.KERNEL
            assert firmware_vm.boot_mode == BootMode.FIRMWARE
        finally:
            from uuid import UUID

            for vid in vm_ids:
                await delete_vm.asyncio_detailed(client=c, vm_id=UUID(vid))


# disk attachment with logical_name


@pytest.fixture
async def storage_pool_and_objects(client):
    """Create a local storage pool with 3 storage objects for disk tests."""
    async with client as c:
        # Find a host to attach the pool to
        hosts = await list_hosts.asyncio(client=c)
        assert hosts and len(hosts) > 0, "No hosts registered"
        host = hosts[0]

        pool = NewStoragePool(
            name="e2e-disk-test-pool",
            pool_type=StoragePoolType.LOCAL,
            config={"path": "/var/lib/qarax/e2e-disk-test"},
        )
        pool_id_raw = await create_pool.asyncio(client=c, body=pool)
        pool_id = str(pool_id_raw).strip('"')

        from uuid import UUID

        await attach_pool_host.asyncio_detailed(
            client=c,
            pool_id=UUID(pool_id),
            body=AttachHostRequest(host_id=host.id, bridge_name="unused"),
        )

        object_ids = []
        for i in range(3):
            obj = NewStorageObject(
                name=f"e2e-disk-obj-{i}",
                storage_pool_id=pool_id,
                object_type=StorageObjectType.DISK,
                size_bytes=1073741824,
            )
            obj_id_raw = await create_storage_object.asyncio(client=c, body=obj)
            object_ids.append(str(obj_id_raw).strip('"'))

        yield c, pool_id, object_ids

        # Cleanup
        for oid in object_ids:
            try:
                from uuid import UUID

                await delete_storage_object.asyncio_detailed(client=c, object_id=oid)
            except Exception:
                pass
        try:
            await delete_pool.asyncio_detailed(client=c, pool_id=pool_id)
        except Exception:
            pass


@pytest.mark.asyncio
async def test_attach_disk_auto_logical_name(storage_pool_and_objects):
    """Attaching a disk without logical_name should auto-generate 'disk0'."""
    c, _pool_id, object_ids = storage_pool_and_objects
    new_vm = NewVm(
        name="e2e-disk-auto",
        hypervisor=Hypervisor.CLOUD_HV,
        boot_vcpus=1,
        max_vcpus=1,
        memory_size=268435456,
        config={},
    )
    result = await create_vm.asyncio(client=c, body=new_vm)
    vm_id_str = str(result if isinstance(result, str) else result.vm_id).strip('"')

    try:
        from uuid import UUID

        req = AttachDiskRequest(storage_object_id=UUID(object_ids[0]))
        disk = await attach_disk_api.asyncio(client=c, vm_id=UUID(vm_id_str), body=req)
        assert disk is not None
        assert disk.logical_name == "disk0"
        assert disk.device_path == "/dev/disk0"
    finally:
        from uuid import UUID

        await delete_vm.asyncio_detailed(client=c, vm_id=UUID(vm_id_str))


@pytest.mark.asyncio
async def test_attach_disk_explicit_logical_name(storage_pool_and_objects):
    """Attaching a disk with a logical_name should use the provided name."""
    c, _pool_id, object_ids = storage_pool_and_objects
    new_vm = NewVm(
        name="e2e-disk-explicit",
        hypervisor=Hypervisor.CLOUD_HV,
        boot_vcpus=1,
        max_vcpus=1,
        memory_size=268435456,
        config={},
    )
    result = await create_vm.asyncio(client=c, body=new_vm)
    vm_id_str = str(result if isinstance(result, str) else result.vm_id).strip('"')

    try:
        from uuid import UUID

        req = AttachDiskRequest(
            storage_object_id=UUID(object_ids[0]),
            logical_name="rootfs",
        )
        disk = await attach_disk_api.asyncio(client=c, vm_id=UUID(vm_id_str), body=req)
        assert disk is not None
        assert disk.logical_name == "rootfs"
        assert disk.device_path == "/dev/rootfs"
    finally:
        from uuid import UUID

        await delete_vm.asyncio_detailed(client=c, vm_id=UUID(vm_id_str))


@pytest.mark.asyncio
async def test_attach_multiple_disks_auto_names(storage_pool_and_objects):
    """Attaching multiple disks without names should generate disk0, disk1, disk2."""
    c, _pool_id, object_ids = storage_pool_and_objects
    new_vm = NewVm(
        name="e2e-multi-disk",
        hypervisor=Hypervisor.CLOUD_HV,
        boot_vcpus=1,
        max_vcpus=1,
        memory_size=268435456,
        config={},
    )
    result = await create_vm.asyncio(client=c, body=new_vm)
    vm_id_str = str(result if isinstance(result, str) else result.vm_id).strip('"')

    try:
        from uuid import UUID

        names = []
        for oid in object_ids:
            req = AttachDiskRequest(storage_object_id=UUID(oid))
            disk = await attach_disk_api.asyncio(
                client=c, vm_id=UUID(vm_id_str), body=req
            )
            assert disk is not None
            names.append(disk.logical_name)

        assert names == ["disk0", "disk1", "disk2"]
    finally:
        from uuid import UUID

        await delete_vm.asyncio_detailed(client=c, vm_id=UUID(vm_id_str))
