"""
E2E tests for file transfers scoped to storage pools.

Tests verify the transfer lifecycle:
- Creating a storage pool with a host
- Submitting a local_copy transfer
- Polling until the transfer completes
- Verifying the resulting storage object
"""

import asyncio
import time
import uuid
from uuid import UUID

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_objects import (
    delete as delete_storage_object,
)
from qarax_api_client.api.storage_objects import (
    get as get_storage_object,
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
from qarax_api_client.api.transfers import (
    create as create_transfer,
)
from qarax_api_client.api.transfers import (
    get as get_transfer,
)
from qarax_api_client.api.transfers import (
    list_ as list_transfers,
)
from qarax_api_client.models import (
    NewStoragePool,
    NewTransfer,
    StorageObjectType,
    StoragePoolType,
    TransferStatus,
)
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest

TRANSFER_TIMEOUT = 30

from helpers import AUTH_HEADERS, QARAX_URL, call_api
from helpers import up_hosts as _up_hosts


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


@pytest.mark.asyncio
async def test_local_copy_transfer(client):
    """Test submitting a local file copy transfer and verifying completion."""
    test_id = uuid.uuid4().hex[:8]
    async with client as c:
        # 1. Find the e2e-node host
        hosts = _up_hosts(await list_hosts.asyncio(client=c))
        assert hosts, "No UP hosts registered"
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

        transfer = None
        try:
            await call_api(
                attach_pool_host,
                client=c,
                pool_id=UUID(pool_id_str),
                body=AttachPoolHostRequest(host_id=host.id),
            )

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
            if transfer is not None and transfer.storage_object_id is not None:
                await call_api(
                    delete_storage_object,
                    client=c,
                    object_id=str(transfer.storage_object_id),
                )
            await call_api(delete_pool, client=c, pool_id=pool_id_str)
