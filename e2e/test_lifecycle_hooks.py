"""
E2E tests for lifecycle hooks.

Tests webhook-based lifecycle hooks that fire on VM state transitions.
Uses a simple HTTP mock server to receive and verify webhook payloads.
"""

import asyncio
import json
import os
import platform
import subprocess
import time
import uuid
from http.server import HTTPServer, BaseHTTPRequestHandler
from threading import Thread

import httpx
import pytest
from qarax_api_client import Client
from qarax_api_client.api.vms import (
    create as create_vm,
    get as get_vm,
    start as start_vm,
    stop as stop_vm,
    delete as delete_vm,
)
from qarax_api_client.models import NewVm, VmStatus
from qarax_api_client.models.hypervisor import Hypervisor

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
VM_OPERATION_TIMEOUT = 30


def _default_webhook_host():
    """Return a host address reachable from Docker Compose services."""
    if platform.system() != "Linux":
        return "host.docker.internal"

    for network in ("e2e_default", "bridge"):
        try:
            result = subprocess.run(
                ["docker", "network", "inspect", network],
                check=True,
                capture_output=True,
                text=True,
            )
            config = json.loads(result.stdout)[0]["IPAM"]["Config"]
            gateway = config[0].get("Gateway")
            if gateway:
                return gateway
        except (subprocess.CalledProcessError, json.JSONDecodeError, IndexError, KeyError):
            continue

    return "172.17.0.1"


# ---------------------------------------------------------------------------
# Mock webhook server
# ---------------------------------------------------------------------------

class WebhookCollector:
    """Collects webhook payloads received by the mock server."""

    def __init__(self):
        self.payloads = []

    def add(self, payload):
        self.payloads.append(payload)

    def clear(self):
        self.payloads.clear()

    def wait_for(self, count, timeout=15):
        """Wait until at least `count` payloads have been received."""
        start = time.time()
        while time.time() - start < timeout:
            if len(self.payloads) >= count:
                return self.payloads[:count]
            time.sleep(0.5)
        raise TimeoutError(
            f"Expected {count} webhook(s), got {len(self.payloads)} after {timeout}s"
        )


collector = WebhookCollector()


class WebhookHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length)
        payload = json.loads(body)
        collector.add(payload)
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok")

    def log_message(self, format, *args):
        pass  # Suppress request logging


@pytest.fixture(scope="module")
def webhook_server():
    """Start a mock webhook server on localhost."""
    server = HTTPServer(("0.0.0.0", 0), WebhookHandler)
    port = server.server_address[1]
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    yield port
    server.shutdown()


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

@pytest.fixture
def client():
    return Client(base_url=QARAX_URL)


@pytest.fixture
def http_client():
    return httpx.Client(base_url=QARAX_URL)


async def call_api(endpoint_module, **kwargs):
    asyncio_fn = getattr(endpoint_module, "asyncio", None)
    if callable(asyncio_fn):
        return await asyncio_fn(**kwargs)
    detailed_fn = getattr(endpoint_module, "asyncio_detailed", None)
    if callable(detailed_fn):
        response = await detailed_fn(**kwargs)
        return response.parsed
    raise AttributeError(f"{endpoint_module.__name__} has no async entrypoint")


async def wait_for_status(client, vm_id, expected_status, timeout=VM_OPERATION_TIMEOUT):
    start_time = time.time()
    while time.time() - start_time < timeout:
        vm = await call_api(get_vm, client=client, vm_id=vm_id)
        if vm is not None and vm.status == expected_status:
            return vm
        await asyncio.sleep(0.5)
    vm = await call_api(get_vm, client=client, vm_id=vm_id)
    status = vm.status if vm is not None else "unknown (VM not found)"
    raise TimeoutError(
        f"VM {vm_id} did not reach {expected_status} in {timeout}s. Got: {status}"
    )


# ---------------------------------------------------------------------------
# Hook CRUD helpers (using httpx until SDK is regenerated)
# ---------------------------------------------------------------------------

def create_hook(http_client, name, url, scope="global", scope_value=None, events=None):
    body = {"name": name, "url": url, "scope": scope}
    if scope_value:
        body["scope_value"] = scope_value
    if events:
        body["events"] = events
    resp = http_client.post("/hooks", json=body)
    assert resp.status_code == 201, f"create hook failed: {resp.text}"
    return resp.text.strip().strip('"')


def list_hooks(http_client, name=None):
    params = {"name": name} if name else {}
    resp = http_client.get("/hooks", params=params)
    assert resp.status_code == 200
    return resp.json()


def get_hook(http_client, hook_id):
    resp = http_client.get(f"/hooks/{hook_id}")
    assert resp.status_code == 200
    return resp.json()


def update_hook(http_client, hook_id, **kwargs):
    resp = http_client.patch(f"/hooks/{hook_id}", json=kwargs)
    assert resp.status_code == 200
    return resp.json()


def delete_hook(http_client, hook_id):
    resp = http_client.delete(f"/hooks/{hook_id}")
    assert resp.status_code == 204


def list_executions(http_client, hook_id):
    resp = http_client.get(f"/hooks/{hook_id}/executions")
    assert resp.status_code == 200
    return resp.json()


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

class TestHookCrud:
    """Test lifecycle hook CRUD operations."""

    def test_create_and_get(self, http_client):
        name = f"test-hook-{uuid.uuid4().hex[:8]}"
        hook_id = create_hook(http_client, name=name, url="https://example.com/hook")

        hook = get_hook(http_client, hook_id)
        assert hook["name"] == name
        assert hook["url"] == "https://example.com/hook"
        assert hook["scope"] == "global"
        assert hook["active"] is True

        # Cleanup
        delete_hook(http_client, hook_id)

    def test_list(self, http_client):
        name = f"test-hook-list-{uuid.uuid4().hex[:8]}"
        hook_id = create_hook(http_client, name=name, url="https://example.com/hook")

        hooks = list_hooks(http_client, name=name)
        assert len(hooks) == 1
        assert hooks[0]["name"] == name

        delete_hook(http_client, hook_id)

    def test_update(self, http_client):
        name = f"test-hook-update-{uuid.uuid4().hex[:8]}"
        hook_id = create_hook(http_client, name=name, url="https://example.com/old")

        updated = update_hook(http_client, hook_id, url="https://example.com/new", active=False)
        assert updated["url"] == "https://example.com/new"
        assert updated["active"] is False

        delete_hook(http_client, hook_id)

    def test_update_can_clear_nullable_fields(self, http_client):
        name = f"test-hook-clear-{uuid.uuid4().hex[:8]}"
        hook_id = create_hook(
            http_client,
            name=name,
            url="https://example.com/hook",
            scope="tag",
            scope_value="blue",
        )
        update_hook(http_client, hook_id, secret="top-secret")

        updated = update_hook(
            http_client,
            hook_id,
            scope="global",
            scope_value=None,
            secret=None,
        )

        assert updated["scope"] == "global"
        assert updated["scope_value"] is None
        assert updated["secret"] is None

        delete_hook(http_client, hook_id)

    def test_delete(self, http_client):
        name = f"test-hook-delete-{uuid.uuid4().hex[:8]}"
        hook_id = create_hook(http_client, name=name, url="https://example.com/hook")
        delete_hook(http_client, hook_id)

        resp = http_client.get(f"/hooks/{hook_id}")
        assert resp.status_code == 404

    def test_duplicate_name_conflict(self, http_client):
        name = f"test-hook-dup-{uuid.uuid4().hex[:8]}"
        hook_id = create_hook(http_client, name=name, url="https://example.com/hook")

        resp = http_client.post("/hooks", json={"name": name, "url": "https://example.com/other"})
        assert resp.status_code == 409

        delete_hook(http_client, hook_id)

    def test_events_filter(self, http_client):
        name = f"test-hook-events-{uuid.uuid4().hex[:8]}"
        hook_id = create_hook(
            http_client, name=name, url="https://example.com/hook",
            events=["running", "shutdown"]
        )

        hook = get_hook(http_client, hook_id)
        assert set(hook["events"]) == {"running", "shutdown"}

        delete_hook(http_client, hook_id)


@pytest.mark.skipif(
    os.getenv("SKIP_VM_TESTS") == "1",
    reason="VM tests skipped (set SKIP_VM_TESTS=0 to enable)",
)
class TestHookExecution:
    """Test that hooks fire on VM state transitions (requires KVM)."""

    @pytest.mark.asyncio
    async def test_hook_fires_on_vm_lifecycle(self, client, http_client, webhook_server):
        """Create a global hook, run a VM lifecycle, and verify webhooks are received."""
        collector.clear()

        # The webhook URL needs to be reachable from the qarax container.
        # In Docker Compose, the test host is accessible as host.docker.internal or via
        # the gateway IP. Fall back to localhost for local dev.
        webhook_host = os.getenv("WEBHOOK_HOST") or _default_webhook_host()
        webhook_url = f"http://{webhook_host}:{webhook_server}/webhook"

        hook_name = f"e2e-hook-{uuid.uuid4().hex[:8]}"
        hook_id = create_hook(http_client, name=hook_name, url=webhook_url)

        try:
            # Create a VM
            new_vm = NewVm(
                name=f"hook-test-{uuid.uuid4().hex[:8]}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=256 * 1024 * 1024,
            )
            resp = await call_api(create_vm, client=client, body=new_vm)
            vm_id = str(resp.vm_id) if hasattr(resp, "vm_id") else str(resp)

            # Start the VM
            await call_api(start_vm, client=client, vm_id=vm_id)
            await wait_for_status(client, vm_id, VmStatus.RUNNING)

            # Stop the VM
            await call_api(stop_vm, client=client, vm_id=vm_id)
            await wait_for_status(client, vm_id, VmStatus.SHUTDOWN)

            # Delete the VM
            await call_api(delete_vm, client=client, vm_id=vm_id)

            # Wait for webhook deliveries (hook_executor polls every 2s)
            # We expect transitions: created->pending, pending->running, running->shutdown, shutdown->deleted
            # The exact set depends on how many status updates happen during the lifecycle
            payloads = collector.wait_for(2, timeout=20)

            # Verify payload structure
            for p in payloads:
                assert p["event"] == "vm.status_changed"
                assert "vm_id" in p
                assert "previous_status" in p
                assert "new_status" in p
                assert "timestamp" in p

            # Check executions via API
            executions = list_executions(http_client, hook_id)
            assert len(executions) >= 2
            delivered = [e for e in executions if e["status"] == "delivered"]
            assert len(delivered) >= 2

        finally:
            delete_hook(http_client, hook_id)
