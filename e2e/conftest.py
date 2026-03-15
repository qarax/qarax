"""
E2E test configuration and shared fixtures.

Registers the qarax-node as a host and sets it to UP before running any tests.
This is required for VM scheduling (the control plane picks a host in UP state).
"""

import os
import uuid

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import add as add_host
from qarax_api_client.api.hosts import init as init_host
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.models.new_host import NewHost

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
# Address of qarax-node as seen from the qarax control plane (inside docker network)
QARAX_NODE_HOST = os.getenv("QARAX_NODE_HOST", "qarax-node")
QARAX_NODE_PORT = int(os.getenv("QARAX_NODE_PORT", "50051"))


@pytest.fixture(scope="session", autouse=True)
def ensure_host_registered():
    """Register the qarax-node host and initialize it before tests run."""
    client = Client(base_url=QARAX_URL)

    hosts = list_hosts.sync(client=client)
    if hosts is None:
        raise RuntimeError("Failed to list hosts")

    selected_host = next((h for h in hosts if h.address == QARAX_NODE_HOST), None)
    host_id = selected_host.id if selected_host is not None else None

    if host_id is None:
        # Not registered yet — register it now
        new_host = NewHost(
            name="e2e-node",
            address=QARAX_NODE_HOST,
            port=QARAX_NODE_PORT,
            host_user="root",
            password="",
        )
        result = add_host.sync_detailed(client=client, body=new_host)
        if result.status_code.value == 201:
            host_id = uuid.UUID(result.parsed.strip())
        else:
            # Could be a 409/422/500 due to stale DB state; re-fetch to find it
            hosts = list_hosts.sync(client=client)
            if hosts is None:
                raise RuntimeError("Failed to list hosts after registration attempt")
            selected_host = next((h for h in hosts if h.address == QARAX_NODE_HOST), None)
            host_id = selected_host.id if selected_host is not None else None

    if host_id is None:
        raise RuntimeError(
            f"Could not register or find a host at {QARAX_NODE_HOST}:{QARAX_NODE_PORT}"
        )

    # Initialize the selected host so the scheduler sees a reachable UP host.
    result = init_host.sync_detailed(host_id=host_id, client=client)
    if result.status_code.value != 200:
        raise RuntimeError(f"Failed to initialize host {host_id}: HTTP {result.status_code}")
