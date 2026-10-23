"""Storage lifecycle checks against the API and real node file operations."""

import asyncio
import uuid

import httpx
import pytest

from helpers import AUTH_HEADERS, QARAX_URL


@pytest.mark.asyncio
async def test_pool_capacity_accounts_for_create_resize_and_delete():
    mib = 1024 * 1024
    name = f"e2e-capacity-{uuid.uuid4().hex[:8]}"
    async with httpx.AsyncClient(base_url=QARAX_URL, headers=AUTH_HEADERS, timeout=30) as client:
        hosts = (await client.get("/hosts")).json()
        host = next(h for h in hosts if h["status"] == "up")
        response = await client.post("/storage-pools", json={
            "name": name, "pool_type": "local", "capacity_bytes": 2 * mib,
            "config": {"path": f"/var/lib/qarax/{name}"},
        })
        assert response.status_code == 201, response.text
        pool_id = response.text.strip('"')
        object_id = vm_id = None
        try:
            response = await client.post(f"/storage-pools/{pool_id}/hosts", json={"host_id": host["id"]})
            assert response.status_code == 204, response.text
            response = await client.post(f"/storage-pools/{pool_id}/disks", json={
                "name": "disk", "size_bytes": mib, "preallocate": True,
            })
            assert response.status_code == 201, response.text
            object_id = response.json()["storage_object_id"]
            response = await client.post("/vms", json={
                "name": name, "hypervisor": "cloud_hv", "boot_vcpus": 1,
                "max_vcpus": 1, "memory_size": 256 * mib, "root_disk_object_id": object_id,
            })
            assert response.status_code == 201, response.text
            vm_id = response.json()
            # The node must have created the VM before it can route a resize.
            response = await client.post(f"/vms/{vm_id}/start")
            assert response.status_code in (200, 202), response.text
            for _ in range(300):
                vm = (await client.get(f"/vms/{vm_id}")).json()
                if vm["status"] == "running":
                    break
                await asyncio.sleep(0.1)
            assert vm["status"] == "running", vm
            response = await client.post(f"/vms/{vm_id}/stop")
            assert response.status_code == 200, response.text
            resize_url = f"/vms/{vm_id}/disks/rootfs/resize"
            response = await client.put(resize_url, json={"new_size_bytes": 3 * mib})
            assert response.status_code == 422, response.text
            response = await client.put(resize_url, json={"new_size_bytes": 2 * mib})
            assert response.status_code == 200, response.text
            assert (await client.get(f"/storage-pools/{pool_id}")).json()["allocated_bytes"] == 2 * mib
            response = await client.post(f"/storage-pools/{pool_id}/disks", json={
                "name": "over-capacity", "size_bytes": mib,
            })
            assert response.status_code == 422, response.text
            assert (await client.delete(f"/vms/{vm_id}")).status_code == 204
            vm_id = None
            assert (await client.delete(f"/storage-objects/{object_id}")).status_code == 204
            object_id = None
            assert (await client.get(f"/storage-pools/{pool_id}")).json()["allocated_bytes"] == 0
        finally:
            if vm_id:
                await client.delete(f"/vms/{vm_id}")
            if object_id:
                await client.delete(f"/storage-objects/{object_id}")
            await client.delete(f"/storage-pools/{pool_id}")
