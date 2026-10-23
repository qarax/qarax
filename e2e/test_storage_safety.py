"""Storage lifecycle checks against the API and real node file operations."""

import asyncio
import os
from pathlib import Path
import subprocess
import uuid

import httpx
import pytest

from helpers import AUTH_HEADERS, QARAX_URL, QARAX_TEST_TOKEN


@pytest.mark.asyncio
@pytest.mark.parametrize("pool_type", ["local", "nfs"])
async def test_pool_capacity_accounts_for_create_resize_and_delete(pool_type):
    mib = 1024 * 1024
    name = f"e2e-capacity-{uuid.uuid4().hex[:8]}"
    async with httpx.AsyncClient(base_url=QARAX_URL, headers=AUTH_HEADERS, timeout=30) as client:
        hosts = (await client.get("/hosts")).json()
        host = next(h for h in hosts if h["status"] == "up")
        response = await client.post("/storage-pools", json={
            "name": name, "pool_type": pool_type, "capacity_bytes": 2 * mib,
            "config": ({"path": f"/var/lib/qarax/{name}"} if pool_type == "local" else {
                "url": f"{os.getenv('NFS_SERVER_HOST', 'nfs-server')}:{os.getenv('NFS_EXPORT_PATH', '/nfs-export')}",
            }),
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
            disk = (await client.get(f"/storage-objects/{object_id}")).json()
            project = os.getenv("QARAX_COMPOSE_PROJECT", "e2e")
            container = subprocess.check_output([
                "docker", "ps", "-q", "--filter", f"label=com.docker.compose.project={project}",
                "--filter", f"label=com.docker.compose.service={host['address']}",
            ], text=True).strip()
            assert container, "No container found for the disk's creating node"
            disk_path = disk["config"]["path"]
            subprocess.run(["docker", "exec", container, "test", "-f", disk_path], check=True)
            # An externally registered alias must never remove the actual file.
            response = await client.post("/storage-objects", json={
                "name": "external-alias", "storage_pool_id": pool_id, "object_type": "disk",
                "size_bytes": 0, "config": {"path": disk_path},
            })
            assert response.status_code == 201, response.text
            alias_id = response.text.strip('"')
            assert (await client.delete(f"/storage-objects/{alias_id}")).status_code == 204
            subprocess.run(["docker", "exec", container, "test", "-f", disk_path], check=True)
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
            cli = os.getenv("QARAX_CLI", str(Path(__file__).resolve().parents[1] / "target/x86_64-unknown-linux-musl/release/qarax"))
            result = subprocess.run([
                cli, "--server", QARAX_URL, "--token", QARAX_TEST_TOKEN,
                "storage-object", "delete", object_id,
            ], text=True, capture_output=True, check=True)
            assert "Deleted storage object" in result.stdout
            subprocess.run(["docker", "exec", container, "test", "!", "-e", disk_path], check=True)
            object_id = None
            assert (await client.get(f"/storage-pools/{pool_id}")).json()["allocated_bytes"] == 0
        finally:
            if vm_id:
                await client.delete(f"/vms/{vm_id}")
            if object_id:
                await client.delete(f"/storage-objects/{object_id}")
            await client.delete(f"/storage-pools/{pool_id}")
