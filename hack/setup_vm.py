#!/usr/bin/env python3
"""
Set up VM resources via the qarax API.

Called by hack/run-local.sh --with-vm to handle all API interactions:
  - Register host and set it to UP
  - Create storage pool with host_id
  - Transfer kernel and initramfs via local copy
  - Create boot source
  - Delete stale VMs, create and start a fresh one

Outputs key=value pairs on stdout for the calling bash script to eval.
All status messages go to stderr so they don't interfere with output parsing.
"""

import argparse
import sys
import time

import requests


def log(msg: str) -> None:
    """Print status messages to stderr."""
    print(msg, file=sys.stderr)


def api(method: str, path: str, base_url: str, **kwargs) -> requests.Response:
    url = f"{base_url}{path}"
    resp = requests.request(method, url, **kwargs)
    return resp


def find_by_name(items: list[dict], name: str) -> dict | None:
    """Find a resource by name in a list response."""
    for item in items:
        if item.get("name") == name:
            return item
    return None


def ensure_host(base_url: str, name: str, address: str, port: int) -> str:
    """Register host if needed, set to UP, return host_id."""
    resp = api("GET", "/hosts", base_url)
    resp.raise_for_status()
    host = find_by_name(resp.json(), name)

    if host:
        host_id = host["id"]
        log(f"Using existing host: {host_id}")
    else:
        resp = api(
            "POST",
            "/hosts",
            base_url,
            json={
                "name": name,
                "address": address,
                "port": port,
                "host_user": "root",
                "password": "",
            },
        )
        if resp.status_code == 201:
            host_id = resp.text.strip().strip('"')
            log(f"Host registered: {host_id}")
        elif resp.status_code == 409:
            # Conflict — host already exists (by name or address). Re-fetch and find it.
            resp2 = api("GET", "/hosts", base_url)
            resp2.raise_for_status()
            hosts = resp2.json()
            host = find_by_name(hosts, name)
            if host is None:
                # Name didn't match — conflict was on address; find by address
                host = next((h for h in hosts if h.get("address") == address), None)
            if host is None:
                raise RuntimeError(
                    f"Host 409 conflict but could not find host by name={name!r} or address={address!r}"
                )
            host_id = host["id"]
            log(f"Host already registered: {host_id}")
        else:
            resp.raise_for_status()

    # Init the host (gRPC probe + set to UP). Retry a few times since
    # qarax-node may still be finishing startup even after the health check passes.
    max_retries = 5
    for attempt in range(1, max_retries + 1):
        resp = api("POST", f"/hosts/{host_id}/init", base_url)
        if resp.status_code == 200:
            log("Host initialized and set to UP")
            return host_id
        log(f"Host init attempt {attempt}/{max_retries} failed: {resp.status_code} {resp.text.strip()}")
        if attempt < max_retries:
            time.sleep(3)

    log("ERROR: Could not initialize host after retries — qarax-node may be unreachable")
    sys.exit(1)


def ensure_overlaybd_pool(base_url: str, name: str, registry_url: str) -> str:
    """Get or create an overlaybd storage pool, return pool_id."""
    resp = api("GET", "/storage-pools", base_url)
    resp.raise_for_status()
    pool = find_by_name(resp.json(), name)

    if pool:
        log(f"Using existing overlaybd pool: {pool['id']}")
        return pool["id"]

    resp = api(
        "POST",
        "/storage-pools",
        base_url,
        json={
            "name": name,
            "pool_type": "overlay_bd",
            "config": {"url": registry_url},
        },
    )
    resp.raise_for_status()
    pool_id = resp.text.strip().strip('"')
    log(f"OverlayBD storage pool created: {pool_id}")

    # The server attaches UP hosts to new pools asynchronously in the background.
    # Wait for the attachment to complete (needed for import operations).
    log("Waiting for host attachment to storage pool...")
    time.sleep(5)

    return pool_id


def ensure_pool(base_url: str, name: str, host_id: str, path: str) -> str:
    """Get or create a storage pool, return pool_id."""
    resp = api("GET", "/storage-pools", base_url)
    resp.raise_for_status()
    pool = find_by_name(resp.json(), name)

    if pool:
        log(f"Using existing storage pool: {pool['id']}")
        return pool["id"]

    resp = api(
        "POST",
        "/storage-pools",
        base_url,
        json={
            "name": name,
            "pool_type": "local",
            "host_id": host_id,
            "config": {"path": path},
        },
    )
    resp.raise_for_status()
    pool_id = resp.text.strip().strip('"')
    log(f"Storage pool created: {pool_id}")
    return pool_id


def ensure_storage_object(
    base_url: str,
    pool_id: str,
    name: str,
    source: str,
    object_type: str,
    timeout: int = 60,
) -> str:
    """Get existing storage object or create one via transfer. Return object_id."""
    resp = api("GET", "/storage-objects", base_url)
    resp.raise_for_status()
    obj = find_by_name(resp.json(), name)
    if obj:
        log(f"Using existing storage object: {name} ({obj['id']})")
        return obj["id"]

    # Submit transfer
    log(f"Transferring {name}...")
    resp = api(
        "POST",
        f"/storage-pools/{pool_id}/transfers",
        base_url,
        json={
            "name": name,
            "source": source,
            "object_type": object_type,
        },
    )
    if resp.status_code != 202:
        log(f"Failed to submit transfer for {name}: {resp.status_code} {resp.text}")
        sys.exit(1)

    transfer = resp.json()
    transfer_id = transfer["id"]
    log(f"Transfer submitted: {transfer_id}")

    # Poll until completed
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        resp = api("GET", f"/storage-pools/{pool_id}/transfers/{transfer_id}", base_url)
        resp.raise_for_status()
        transfer = resp.json()

        if transfer["status"] == "completed":
            object_id = transfer["storage_object_id"]
            bytes_written = transfer.get("transferred_bytes", 0)
            log(f"Transfer completed: {name} -> {object_id} ({bytes_written} bytes)")
            return object_id
        elif transfer["status"] == "failed":
            log(
                f"Transfer failed for {name}: {transfer.get('error_message', 'unknown')}"
            )
            sys.exit(1)

        time.sleep(1)

    log(f"Transfer timed out for {name}")
    sys.exit(1)


def ensure_boot_source(
    base_url: str,
    name: str,
    kernel_id: str,
    initramfs_id: str | None,
    kernel_params: str,
) -> str:
    """Get or create a boot source, return boot_source_id."""
    resp = api("GET", "/boot-sources", base_url)
    resp.raise_for_status()
    bs = find_by_name(resp.json(), name)
    if bs:
        log(f"Using existing boot source: {bs['id']}")
        return bs["id"]

    body = {
        "name": name,
        "description": "Local dev boot source",
        "kernel_image_id": kernel_id,
        "kernel_params": kernel_params,
    }
    if initramfs_id:
        body["initrd_image_id"] = initramfs_id

    resp = api("POST", "/boot-sources", base_url, json=body)
    resp.raise_for_status()
    boot_id = resp.text.strip().strip('"')
    log(f"Boot source created: {boot_id}")
    return boot_id


def delete_existing_vm(base_url: str, name: str) -> None:
    """Delete a VM by name if it exists (to avoid stale state)."""
    resp = api("GET", "/vms", base_url)
    resp.raise_for_status()
    vm = find_by_name(resp.json(), name)
    if not vm:
        return

    vm_id = vm["id"]
    log(f"Deleting existing VM: {vm_id}")
    resp = api("DELETE", f"/vms/{vm_id}", base_url)
    if resp.status_code in (200, 204):
        log("Existing VM deleted")
        time.sleep(1)
    else:
        log(f"Failed to delete existing VM (HTTP {resp.status_code}), continuing")


def ensure_instance_type(
    base_url: str,
    name: str,
    boot_vcpus: int,
    max_vcpus: int,
    memory_size: int,
    accelerator_config: dict | None = None,
) -> str:
    """Get or create an instance type, return instance_type_id."""
    resp = api("GET", "/instance-types", base_url)
    resp.raise_for_status()
    it = find_by_name(resp.json(), name)
    if it:
        log(f"Using existing instance type: {it['id']}")
        return it["id"]

    body = {
        "name": name,
        "boot_vcpus": boot_vcpus,
        "max_vcpus": max_vcpus,
        "memory_size": memory_size,
    }
    if accelerator_config:
        body["accelerator_config"] = accelerator_config

    resp = api("POST", "/instance-types", base_url, json=body)
    resp.raise_for_status()
    it_id = resp.text.strip().strip('"')
    log(f"Instance type created: {it_id}")
    return it_id


def create_and_start_vm(
    base_url: str,
    name: str,
    boot_source_id: str,
    memory_size: int = 256 * 1024 * 1024,
    boot_vcpus: int = 1,
    max_vcpus: int = 1,
    networks: list[dict] | None = None,
) -> str | None:
    """Create and start a VM, return vm_id or None on failure."""
    body = {
        "name": name,
        "hypervisor": "cloud_hv",
        "boot_vcpus": boot_vcpus,
        "max_vcpus": max_vcpus,
        "memory_size": memory_size,
        "boot_source_id": boot_source_id,
    }
    if networks:
        body["networks"] = networks

    resp = api("POST", "/vms", base_url, json=body)
    if resp.status_code != 201:
        log(f"Failed to create VM: {resp.status_code} {resp.text}")
        return None

    vm_id = resp.text.strip().strip('"')
    log(f"VM created: {vm_id}")

    resp = api("POST", f"/vms/{vm_id}/start", base_url)
    if resp.status_code == 202:
        job_id = resp.json().get("job_id")
        log(f"VM start accepted, job: {job_id}")
        # Poll job until completion
        if job_id:
            deadline = time.monotonic() + 120
            while time.monotonic() < deadline:
                jr = api("GET", f"/jobs/{job_id}", base_url)
                jr.raise_for_status()
                job = jr.json()
                if job["status"] == "completed":
                    log("VM started successfully")
                    break
                elif job["status"] == "failed":
                    log(f"VM start job failed: {job.get('error', 'unknown')}")
                    break
                time.sleep(2)
            else:
                log("VM start job timed out")
    else:
        log(f"Failed to start VM: {resp.status_code} {resp.text}")

    return vm_id


def main() -> None:
    parser = argparse.ArgumentParser(description="Set up qarax VM resources")
    parser.add_argument("--api-url", default="http://localhost:8000")
    parser.add_argument("--host-name", default="local-node")
    parser.add_argument("--host-address", default="qarax-node")
    parser.add_argument("--host-port", type=int, default=50051)
    parser.add_argument("--pool-path", default="/var/lib/qarax/images")
    parser.add_argument("--overlaybd-registry-url", default="http://registry:5000")
    parser.add_argument("--kernel-path", required=True)
    parser.add_argument("--initramfs-path", default="")
    parser.add_argument("--cmdline", default="console=ttyS0")
    parser.add_argument(
        "--gpu-instance-type",
        action="store_true",
        help="Create a GPU-enabled instance type (gpu.small: 4 vCPUs, 8G, 1 nvidia GPU)",
    )
    parser.add_argument(
        "--skip-vm",
        action="store_true",
        help="Only set up host and storage pools; skip local pool, transfers, boot source, and VM",
    )
    parser.add_argument(
        "--skip-host",
        action="store_true",
        help="Skip host registration and init (host already managed externally, e.g. libvirt VM)",
    )
    args = parser.parse_args()

    base = args.api_url

    host_id = None
    if not args.skip_host:
        host_id = ensure_host(base, args.host_name, args.host_address, args.host_port)
        print(f"HOST_ID={host_id}")

    overlaybd_pool_id = ensure_overlaybd_pool(base, "overlaybd-pool", args.overlaybd_registry_url)
    print(f"OVERLAYBD_POOL_ID={overlaybd_pool_id}")

    if args.gpu_instance_type:
        gpu_it_id = ensure_instance_type(
            base,
            "gpu.small",
            boot_vcpus=4,
            max_vcpus=4,
            memory_size=8 * 1024 * 1024 * 1024,
            accelerator_config={"gpu_count": 1, "gpu_vendor": "nvidia"},
        )
        print(f"GPU_INSTANCE_TYPE_ID={gpu_it_id}")

    if args.skip_vm:
        return

    if not host_id:
        log("ERROR: --skip-host requires --skip-vm (host_id needed for pool/VM creation)")
        sys.exit(1)

    pool_id = ensure_pool(base, "local-pool", host_id, args.pool_path)

    kernel_id = ensure_storage_object(
        base,
        pool_id,
        "example-kernel",
        args.kernel_path,
        "kernel",
    )

    initramfs_id = None
    if args.initramfs_path:
        initramfs_id = ensure_storage_object(
            base,
            pool_id,
            "example-initramfs",
            args.initramfs_path,
            "initrd",
        )

    boot_id = ensure_boot_source(
        base,
        "example-boot",
        kernel_id,
        initramfs_id,
        args.cmdline,
    )

    delete_existing_vm(base, "example-vm")
    vm_id = create_and_start_vm(
        base,
        "example-vm",
        boot_id,
        memory_size=256 * 1024 * 1024,
        networks=[{"id": "net0", "mac": "52:54:00:12:34:56"}],
    )

    print(f"POOL_ID={pool_id}")
    print(f"KERNEL_ID={kernel_id}")
    if initramfs_id:
        print(f"INITRAMFS_ID={initramfs_id}")
    print(f"BOOT_SOURCE_ID={boot_id}")
    if vm_id:
        print(f"VM_ID={vm_id}")


if __name__ == "__main__":
    main()
