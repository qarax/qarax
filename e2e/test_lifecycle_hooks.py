"""
E2E tests for lifecycle hooks.

Tests webhook-based lifecycle hooks that fire on VM state transitions.
Uses a simple HTTP mock server to receive and verify webhook payloads.
"""

import json
import os
import platform
import subprocess
import time
import uuid
from http.server import BaseHTTPRequestHandler, HTTPServer
from threading import Thread

import httpx
import pytest
from qarax_api_client import Client
from qarax_api_client.api.vms import (
    create as create_vm,
)
from qarax_api_client.api.vms import (
    delete as delete_vm,
)
from qarax_api_client.api.vms import (
    start as start_vm,
)
from qarax_api_client.api.vms import (
    stop as stop_vm,
)
from qarax_api_client.models import NewVm, VmStatus
from qarax_api_client.models.hypervisor import Hypervisor

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")


def _default_webhook_host():
    """Return a host address reachable from Docker Compose services."""
    if platform.system() != "Linux":
        return "host.docker.internal"

    for network in ("e2e_default", "bridge"):
        try:
            result = docker("network", "inspect", network)
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

    def wait_for_vm_statuses(self, vm_id, statuses, timeout=15):
        """Wait until payloads for `vm_id` cover every new_status in `statuses`.

        Returns that VM's payloads.
        """
        deadline = time.time() + timeout
        while True:
            payloads = [p for p in list(self.payloads) if p.get("vm_id") == vm_id]
            seen = {p.get("new_status") for p in payloads}
            if statuses <= seen:
                return payloads
            if time.time() >= deadline:
                raise TimeoutError(
                    f"Expected webhooks for VM {vm_id} with new_status {sorted(statuses)}, "
                    f"got {sorted(s for s in seen if s)} after {timeout}s"
                )
            time.sleep(0.5)


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
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


@pytest.fixture
def http_client():
    return httpx.Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


from helpers import AUTH_HEADERS, call_api, docker, wait_for_status

# ---------------------------------------------------------------------------
# Hook helpers (using httpx until SDK is regenerated)
# ---------------------------------------------------------------------------

def create_hook(http_client, name, url):
    resp = http_client.post("/hooks", json={"name": name, "url": url, "scope": "global"})
    assert resp.status_code == 201, f"create hook failed: {resp.text}"
    return resp.text.strip().strip('"')


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
        vm_id = None

        async with client as c:
            try:
                new_vm = NewVm(
                    name=f"hook-test-{uuid.uuid4().hex[:8]}",
                    hypervisor=Hypervisor.CLOUD_HV,
                    boot_vcpus=1,
                    max_vcpus=1,
                    memory_size=256 * 1024 * 1024,
                )
                vm_id = str(await call_api(create_vm, client=c, body=new_vm)).strip('"')

                await call_api(start_vm, client=c, vm_id=vm_id)
                await wait_for_status(c, vm_id, VmStatus.RUNNING)

                await call_api(stop_vm, client=c, vm_id=vm_id)
                await wait_for_status(c, vm_id, VmStatus.SHUTDOWN)

                # The hook is global, so only count deliveries for this test's VM.
                # hook_executor polls every 2s.
                payloads = collector.wait_for_vm_statuses(
                    vm_id, {"running", "shutdown"}, timeout=20
                )
                for p in payloads:
                    assert p["event"] == "vm.status_changed"
                    assert "previous_status" in p
                    assert "timestamp" in p

                executions = [
                    e for e in list_executions(http_client, hook_id) if e["vm_id"] == vm_id
                ]
                delivered = {
                    e["new_status"] for e in executions if e["status"] == "delivered"
                }
                assert {"running", "shutdown"} <= delivered, executions
                # The successful delivery itself counts as an attempt.
                assert all(
                    e["attempt_count"] >= 1 for e in executions if e["status"] == "delivered"
                ), executions
            finally:
                if vm_id is not None:
                    await call_api(delete_vm, client=c, vm_id=vm_id)
                delete_hook(http_client, hook_id)
