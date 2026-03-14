"""
E2E test configuration and shared fixtures.

Registers the qarax-node as a host and sets it to UP before running any tests.
This is required for VM scheduling (the control plane picks a host in UP state).
"""

import os

import httpx
import pytest

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
DEFAULT_QARAX_NODE_HOST = "qarax-node"
# Address of qarax-node as seen from the qarax control plane (inside docker network)
QARAX_NODE_HOST = os.getenv("QARAX_NODE_HOST", DEFAULT_QARAX_NODE_HOST)
QARAX_NODE_PORT = int(os.getenv("QARAX_NODE_PORT", "50051"))
QARAX_NODE_HOST_EXPLICIT = "QARAX_NODE_HOST" in os.environ


def _find_host_by_address(hosts: list[dict], address: str) -> dict | None:
    """Return the first host with the given address, or None."""
    for host in hosts:
        if host.get("address") == address:
            return host
    return None


def _host_is_initialized(host: dict) -> bool:
    return bool(host.get("cloud_hypervisor_version")) or host.get("total_cpus") is not None


def _select_target_host(hosts: list[dict]) -> tuple[dict | None, dict | None]:
    configured_host = _find_host_by_address(hosts, QARAX_NODE_HOST)

    if QARAX_NODE_HOST_EXPLICIT or QARAX_NODE_HOST != DEFAULT_QARAX_NODE_HOST:
        return configured_host, None

    initialized_host = next(
        (
            host
            for host in hosts
            if host.get("address") != DEFAULT_QARAX_NODE_HOST and _host_is_initialized(host)
        ),
        None,
    )
    stale_default_host = _find_host_by_address(hosts, DEFAULT_QARAX_NODE_HOST)

    if initialized_host and (stale_default_host is None or not _host_is_initialized(stale_default_host)):
        return initialized_host, stale_default_host

    return configured_host, None


def _set_host_status(client: httpx.Client, host_id: str, status: str) -> None:
    resp = client.patch(f"/hosts/{host_id}", json={"status": status})
    if resp.status_code not in (200, 204):
        raise RuntimeError(f"Failed to set host {host_id} to {status}: HTTP {resp.status_code} — {resp.text}")


def _init_host(client: httpx.Client, host_id: str) -> None:
    resp = client.post(f"/hosts/{host_id}/init")
    if resp.status_code != 200:
        raise RuntimeError(f"Failed to initialize host {host_id}: HTTP {resp.status_code} — {resp.text}")


@pytest.fixture(scope="session", autouse=True)
def ensure_host_registered():
    """Register the qarax-node host and initialize it before tests run."""
    with httpx.Client(base_url=QARAX_URL) as client:
        hosts = client.get("/hosts")
        hosts.raise_for_status()
        host_list = hosts.json()
        selected_host, stale_default_host = _select_target_host(host_list)
        host_id = selected_host["id"] if selected_host is not None else None

        if host_id is None:
            # Not registered yet — register it now
            resp = client.post(
                "/hosts",
                json={
                    "name": "e2e-node",
                    "address": QARAX_NODE_HOST,
                    "port": QARAX_NODE_PORT,
                    "host_user": "root",
                    "password": "",
                },
            )
            if resp.status_code == 201:
                host_id = resp.text.strip().strip('"')
            else:
                # Could be a 409/422/500 due to stale DB state; re-fetch to find it
                hosts = client.get("/hosts")
                hosts.raise_for_status()
                selected_host = _find_host_by_address(hosts.json(), QARAX_NODE_HOST)
                host_id = selected_host["id"] if selected_host is not None else None

        if host_id is None:
            raise RuntimeError(
                f"Could not register or find a host at {QARAX_NODE_HOST}:{QARAX_NODE_PORT}"
            )

        # Initialize the selected host so the scheduler sees a reachable UP host.
        _init_host(client, host_id)

        if stale_default_host is not None:
            _set_host_status(client, stale_default_host["id"], "down")
