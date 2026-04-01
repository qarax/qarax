"""
E2E tests for file transfers scoped to storage pools.

Tests verify the transfer lifecycle:
- Creating a storage pool with a host
- Submitting a local_copy transfer
- Polling until the transfer completes
- Verifying the resulting storage object
"""

import asyncio
import os
import time

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_pools import (
    create as create_pool,
    delete as delete_pool,
    attach_host as attach_pool_host,
)
from qarax_api_client.api.storage_objects import (
    get as get_storage_object,
    delete as delete_storage_object,
)
from qarax_api_client.api.transfers import (
    create as create_transfer,
    get as get_transfer,
    list_ as list_transfers,
)
from qarax_api_client.models import (
    NewStoragePool,
    StoragePoolType,
    NewTransfer,
    StorageObjectType,
    TransferStatus,
)
from qarax_api_client.models.attach_host_request import AttachHostRequest


QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
TRANSFER_TIMEOUT = 30


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL)


@pytest.mark.asyncio
async def test_local_copy_transfer(client):
    """Test submitting a local file copy transfer and verifying completion."""
    import uuid
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        # 1. Find the e2e-node host
        hosts = await list_hosts.asyncio(client=c)
        assert hosts is not None and len(hosts) > 0, "No hosts registered"
        host = hosts[0]

        # 2. Create a storage pool and attach the host
        new_pool = NewStoragePool(
            name=f"test-transfer-pool-{test_id}",
            pool_type=StoragePoolType.LOCAL,
            config={"path": f"/var/lib/qarax/test-transfers-{test_id}"},
        )
        pool_id = await create_pool.asyncio(client=c, body=new_pool)
        assert pool_id is not None
        pool_id_str = str(pool_id).strip('"')

        from uuid import UUID
        await attach_pool_host.asyncio_detailed(
            client=c,
            pool_id=UUID(pool_id_str),
            body=AttachHostRequest(host_id=host.id, bridge_name="unused"),
        )

        transfer = None
        try:
            # 3. Submit a local_copy transfer
            # The test kernel is already on the node at this path
            new_transfer = NewTransfer(
                name="test-kernel",
                source="/bin/sh",
                object_type=StorageObjectType.KERNEL,
            )
            transfer = await create_transfer.asyncio(
                client=c, pool_id=pool_id_str, body=new_transfer
            )
            assert transfer is not None
            assert transfer.status in (TransferStatus.PENDING, TransferStatus.RUNNING)
            transfer_id_str = str(transfer.id)

            # 4. Poll until completed or failed
            start_time = time.time()
            while time.time() - start_time < TRANSFER_TIMEOUT:
                transfer = await get_transfer.asyncio(
                    client=c, pool_id=pool_id_str, transfer_id=transfer_id_str
                )
                if transfer.status == TransferStatus.COMPLETED:
                    break
                if transfer.status == TransferStatus.FAILED:
                    pytest.fail(
                        f"Transfer failed: {transfer.error_message}"
                    )
                await asyncio.sleep(0.5)
            else:
                pytest.fail(
                    f"Transfer did not complete within {TRANSFER_TIMEOUT}s. "
                    f"Status: {transfer.status}"
                )

            # 5. Verify the transfer result
            assert transfer.storage_object_id is not None
            assert transfer.transferred_bytes > 0

            # 6. Verify the created storage object
            storage_obj = await get_storage_object.asyncio(
                client=c, object_id=str(transfer.storage_object_id)
            )
            assert storage_obj is not None
            assert storage_obj.name == "test-kernel"
            assert storage_obj.object_type == StorageObjectType.KERNEL
            assert storage_obj.config.get("path") == f"/var/lib/qarax/test-transfers-{test_id}/test-kernel"

            # 7. Verify list endpoint
            transfers_list = await list_transfers.asyncio(
                client=c, pool_id=pool_id_str
            )
            assert transfers_list is not None
            assert any(str(t.id) == transfer_id_str for t in transfers_list)

        finally:
            # Cleanup: try to delete storage object, then pool.
            # Storage object deletion may fail if the transfer's FK still references it;
            # that's acceptable since unique pool names prevent stale data collisions.
            try:
                if transfer is not None and transfer.storage_object_id is not None:
                    await delete_storage_object.asyncio_detailed(
                        client=c, object_id=str(transfer.storage_object_id)
                    )
            except Exception:
                pass
            try:
                await delete_pool.asyncio_detailed(client=c, pool_id=pool_id_str)
            except Exception:
                pass
