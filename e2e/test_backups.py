"""
E2E tests for the top-level backup API.

These tests cover:
- VM backup create/list/get/restore through /backups
- Control-plane database backup/restore through /backups
"""

import uuid

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.instance_types import create as create_instance_type
from qarax_api_client.api.instance_types import delete as delete_instance_type
from qarax_api_client.api.instance_types import get as get_instance_type
from qarax_api_client.api.storage_pools import attach_host as attach_pool_host
from qarax_api_client.api.storage_pools import create as create_pool
from qarax_api_client.api.storage_pools import delete as delete_pool
from qarax_api_client.api.vms import create as create_vm
from qarax_api_client.api.vms import delete as delete_vm
from qarax_api_client.api.vms import start as start_vm
from qarax_api_client.api.vms import stop as stop_vm
from qarax_api_client.models import (
    HostStatus,
    Hypervisor,
    NewInstanceType,
    NewStoragePool,
    NewVm,
    StoragePoolType,
    VmStatus,
)
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest

from helpers import QARAX_URL, wait_for_status

VM_OPERATION_TIMEOUT = 60


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


@pytest.fixture(scope="module")
def backup_storage_pool():
    """Create a local storage pool backed by the control-plane filesystem."""
    with Client(base_url=QARAX_URL) as c:
        hosts = [h for h in (list_hosts.sync(client=c) or []) if h.status == HostStatus.UP]
        assert hosts, "No UP hosts registered"
        host_id = hosts[0].id

        pool_id_raw = create_pool.sync(
            client=c,
            body=NewStoragePool(
                name=f"e2e-backup-pool-{uuid.uuid4().hex[:8]}",
                pool_type=StoragePoolType.LOCAL,
                config={"path": "/tmp/qarax-e2e-backups"},
            ),
        )
        assert pool_id_raw is not None, "Failed to create backup storage pool"
        pool_id = uuid.UUID(str(pool_id_raw).strip('"'))

        attach_pool_host.sync_detailed(
            client=c,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=host_id),
        )

        yield pool_id

        delete_pool.sync_detailed(client=c, pool_id=pool_id)


@pytest.mark.asyncio
async def test_vm_backup_lifecycle(client, backup_storage_pool):
    async with client as c:
        httpx_client = c.get_async_httpx_client()
        new_vm = NewVm(
            name=f"e2e-backup-vm-{uuid.uuid4().hex[:8]}",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
        )
        vm_id = uuid.UUID(str(await create_vm.asyncio(client=c, body=new_vm)).strip('"'))

        try:
            await start_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT)

            create_resp = await httpx_client.post(
                f"{QARAX_URL}/backups",
                json={
                    "backup_type": "vm",
                    "vm_id": str(vm_id),
                    "storage_pool_id": str(backup_storage_pool),
                    "name": f"e2e-vm-backup-{uuid.uuid4().hex[:8]}",
                },
            )
            assert create_resp.status_code == 201, create_resp.text
            backup = create_resp.json()
            assert backup["backup_type"] == "vm"
            assert backup["status"] == "ready"
            assert backup["vm_id"] == str(vm_id)
            assert backup["snapshot_id"] is not None

            list_resp = await httpx_client.get(
                f"{QARAX_URL}/backups", params={"name": backup["name"]}
            )
            assert list_resp.status_code == 200, list_resp.text
            backups = list_resp.json()
            assert len(backups) == 1
            assert backups[0]["id"] == backup["id"]

            get_resp = await httpx_client.get(f"{QARAX_URL}/backups/{backup['id']}")
            assert get_resp.status_code == 200, get_resp.text
            assert get_resp.json()["id"] == backup["id"]

            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
            await wait_for_status(c, vm_id, VmStatus.SHUTDOWN, timeout=VM_OPERATION_TIMEOUT)

            restore_resp = await httpx_client.post(
                f"{QARAX_URL}/backups/{backup['id']}/restore"
            )
            assert restore_resp.status_code == 200, restore_resp.text
            restored = restore_resp.json()
            assert restored["backup_id"] == backup["id"]
            assert restored["vm_id"] == str(vm_id)

            await wait_for_status(c, vm_id, VmStatus.RUNNING, timeout=VM_OPERATION_TIMEOUT)
            await stop_vm.asyncio_detailed(client=c, vm_id=vm_id)
        finally:
            await delete_vm.asyncio_detailed(client=c, vm_id=vm_id)


@pytest.mark.asyncio
async def test_database_backup_restore(client, backup_storage_pool):
    async with client as c:
        httpx_client = c.get_async_httpx_client()
        before_name = f"e2e-before-backup-{uuid.uuid4().hex[:8]}"
        after_name = f"e2e-after-backup-{uuid.uuid4().hex[:8]}"

        before_id = await create_instance_type.asyncio(
            client=c,
            body=NewInstanceType(
                name=before_name,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=256 * 1024 * 1024,
            ),
        )
        assert before_id is not None

        try:
            create_resp = await httpx_client.post(
                f"{QARAX_URL}/backups",
                json={
                    "backup_type": "database",
                    "storage_pool_id": str(backup_storage_pool),
                    "name": f"e2e-db-backup-{uuid.uuid4().hex[:8]}",
                },
            )
            assert create_resp.status_code == 201, create_resp.text
            backup = create_resp.json()
            assert backup["backup_type"] == "database"
            assert backup["status"] == "ready"

            after_id = await create_instance_type.asyncio(
                client=c,
                body=NewInstanceType(
                    name=after_name,
                    boot_vcpus=1,
                    max_vcpus=1,
                    memory_size=512 * 1024 * 1024,
                ),
            )
            assert after_id is not None

            restore_resp = await httpx_client.post(
                f"{QARAX_URL}/backups/{backup['id']}/restore"
            )
            assert restore_resp.status_code == 200, restore_resp.text
            restored = restore_resp.json()
            assert restored["backup_id"] == backup["id"]
            assert restored["backup_type"] == "database"
            assert restored["database_name"] == "qarax"

            restored_before = await get_instance_type.asyncio(
                client=c, instance_type_id=str(before_id)
            )
            assert restored_before is not None
            assert restored_before.name == before_name

            after_resp = await get_instance_type.asyncio_detailed(
                client=c, instance_type_id=str(after_id)
            )
            assert after_resp.status_code.value == 404, after_resp.content
        finally:
            await delete_instance_type.asyncio_detailed(
                client=c, instance_type_id=str(before_id)
            )
