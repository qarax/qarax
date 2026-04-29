"""Shared helpers for qarax e2e tests."""

import asyncio
import os
import time

from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.models import HostStatus, VmStatus

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
VM_OPERATION_TIMEOUT = 30


async def call_api(endpoint_module, **kwargs):
    """Call a generated SDK endpoint in a version-tolerant way."""
    asyncio_fn = getattr(endpoint_module, "asyncio", None)
    if callable(asyncio_fn):
        return await asyncio_fn(**kwargs)

    detailed_fn = getattr(endpoint_module, "asyncio_detailed", None)
    if callable(detailed_fn):
        response = await detailed_fn(**kwargs)
        status_code = getattr(response, "status_code", None)
        code = getattr(status_code, "value", status_code)
        if code is not None and code >= 400:
            body = getattr(response, "content", b"")
            if isinstance(body, bytes):
                body = body.decode(errors="replace")
            raise AssertionError(
                f"{endpoint_module.__name__} failed with HTTP {code}: {body}"
            )
        return response.parsed

    raise AttributeError(f"{endpoint_module.__name__} has no async entrypoint")


async def call_api_detailed(endpoint_module, **kwargs):
    """Call a generated SDK endpoint and return the full response."""
    detailed_fn = getattr(endpoint_module, "asyncio_detailed", None)
    if callable(detailed_fn):
        return await detailed_fn(**kwargs)
    raise AttributeError(f"{endpoint_module.__name__} has no asyncio_detailed entrypoint")


def up_hosts(hosts):
    """Return only hosts in UP state."""
    return [h for h in (hosts or []) if h.status == HostStatus.UP]


async def wait_for_status(
    client,
    vm_id,
    expected_status: VmStatus,
    timeout: int = VM_OPERATION_TIMEOUT,
):
    """Poll until a VM reaches the expected status or the timeout expires."""
    start = time.time()
    while time.time() - start < timeout:
        vm = await call_api(get_vm, client=client, vm_id=vm_id)
        if vm is not None and vm.status == expected_status:
            return vm
        await asyncio.sleep(0.5)

    vm = await call_api(get_vm, client=client, vm_id=vm_id)
    current = vm.status if vm is not None else "unknown (VM not found)"
    raise TimeoutError(
        f"VM {vm_id} did not reach {expected_status} within {timeout}s. Current: {current}"
    )
