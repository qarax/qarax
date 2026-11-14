"""
E2E tests for real bootc node deploy and upgrade on a Cloud Hypervisor VM.

These tests exercise the full deploy/upgrade lifecycle against a real
bootc-managed system, the appliance image, booted as a microVM by the
bootc-vm service:

  1. POST /hosts/{id}/deploy  — SSH into the VM, run real bootc switch, reboot
  2. POST /hosts/{id}/upgrade — same flow using stored credentials

The bootc-vm container boots the appliance image plus a test layer
(Containerfile.bootc-node) with qarax-node running as a systemd service.

Version tracking works through the systemd EnvironmentFile mechanism:
  - 10.0.2.2:5000/qarax-node-test:0.1.0     sets QARAX_NODE_VERSION=0.1.0
  - 10.0.2.2:5000/qarax-node-test:0.2.0-test sets QARAX_NODE_VERSION=0.2.0-test

After a real bootc switch + systemctl reboot, the VM boots the new image and
qarax-node reports the version from the EnvironmentFile.

Prerequisites (handled by run_e2e_tests.sh):
  - bootc-vm-overlay.qcow2 over a `bootc install to-disk` of that image
    (build-bootc-node.sh)
  - registry:5000/qarax-node-test:0.1.0     pushed to local registry
  - registry:5000/qarax-node-test:0.2.0-test pushed to local registry
"""

import os
import time
import uuid

import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import add as add_host
from qarax_api_client.api.hosts import delete as delete_host
from qarax_api_client.api.hosts import deploy as deploy_host
from qarax_api_client.api.hosts import init as init_host
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.hosts import node_upgrade
from qarax_api_client.models import (
    DeployHostRequest,
    HostStatus,
    NewHost,
)

from helpers import AUTH_HEADERS

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
# bootc-vm is the container running the bootc node VM
UPGRADE_TEST_NODE_HOST = os.getenv("UPGRADE_TEST_NODE_HOST", "bootc-vm")
UPGRADE_TEST_NODE_PORT = int(os.getenv("UPGRADE_TEST_NODE_PORT", "50051"))

# Registry address as seen from inside the VM (its gateway, where socat
# proxies the compose registry)
REGISTRY_INTERNAL = "10.0.2.2:5000"
NODE_IMAGE_V1 = f"{REGISTRY_INTERNAL}/qarax-node-test:0.1.0"
NODE_IMAGE_V2 = f"{REGISTRY_INTERNAL}/qarax-node-test:0.2.0-test"
EXPECTED_V2_VERSION = "0.2.0-test"

# Real VM operations (bootc switch + reboot) take significantly longer than
# the fake simulation — allow up to 10 minutes per status transition.
DEPLOY_TIMEOUT_SECONDS = 600
HOST_POLL_INTERVAL_SECONDS = 5

# How long to wait for the bootc VM to become reachable after compose up.
VM_BOOT_TIMEOUT_SECONDS = 600

# A test may wait for the VM to boot and then for up to two deploy/upgrade
# cycles, well past the suite-wide pytest-timeout.
pytestmark = pytest.mark.timeout(VM_BOOT_TIMEOUT_SECONDS + 2 * DEPLOY_TIMEOUT_SECONDS + 300)


def _is_truthy(value: str | None) -> bool:
    return (value or "").lower() in {"1", "true", "yes", "on"}


def get_host(client: Client, host_id: uuid.UUID):
    """Fetch a single host by ID via the list endpoint."""
    hosts = list_hosts.sync(client=client)
    assert hosts is not None, "Failed to list hosts"
    match = next((h for h in hosts if h.id == host_id), None)
    assert match is not None, f"Host {host_id} not found"
    return match


def wait_for_host_status(
    client: Client,
    host_id: uuid.UUID,
    expected_status: HostStatus,
    timeout: int = DEPLOY_TIMEOUT_SECONDS,
):
    """Poll until the host reaches expected_status or timeout expires."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        host = get_host(client, host_id)
        if host.status == expected_status:
            return host
        if host.status == HostStatus.INSTALLATION_FAILED:
            raise AssertionError(
                f"Host {host_id} reached installation_failed "
                f"(expected {expected_status}): {host.installation_error}"
            )
        time.sleep(HOST_POLL_INTERVAL_SECONDS)

    host = get_host(client, host_id)
    raise TimeoutError(
        f"Host {host_id} did not reach {expected_status} within {timeout}s. "
        f"Current status: {host.status}"
    )


@pytest.fixture(scope="module")
def upgrade_host_id():
    """
    Register a dedicated host entry pointing at the real bootc VM.

    Retries host init until the VM's gRPC port answers (it may still be booting
    when the test suite starts), so node_version is populated. The address is
    a compose service name that only the control plane can resolve, so there
    is no probe from here.
    """
    if not _is_truthy(os.getenv("ENABLE_BOOTC_UPGRADE_E2E")):
        pytest.skip("bootc upgrade e2e disabled (set ENABLE_BOOTC_UPGRADE_E2E=1 to run)")

    client = Client(base_url=QARAX_URL, headers=AUTH_HEADERS)
    host_name = f"bootc-vm-{uuid.uuid4().hex[:8]}"

    result = add_host.sync_detailed(
        client=client,
        body=NewHost(
            name=host_name,
            address=UPGRADE_TEST_NODE_HOST,
            port=UPGRADE_TEST_NODE_PORT,
            host_user="root",
            credential_ref="env://E2E_UPGRADE_SSH_PASSWORD",
        ),
    )
    assert result.status_code.value == 201, (
        f"Failed to register upgrade test host: HTTP {result.status_code}"
    )
    host_id = uuid.UUID(result.parsed.strip())

    # The VM may still be booting — retry init until gRPC is reachable.
    deadline = time.time() + VM_BOOT_TIMEOUT_SECONDS
    while True:
        init_result = init_host.sync_detailed(host_id=host_id, client=client)
        if init_result.status_code.value == 200:
            break
        if time.time() >= deadline:
            pytest.fail(
                f"bootc-vm gRPC port did not become reachable within "
                f"{VM_BOOT_TIMEOUT_SECONDS}s"
            )
        time.sleep(10)

    yield host_id
    # Remove the host so later tests don't schedule onto it.
    delete_host.sync_detailed(host_id=host_id, client=client)


def test_deploy_sets_last_deployed_image_and_node_version(upgrade_host_id):
    """
    Full deploy flow with v1 image using real bootc switch + systemctl reboot:
      - POST /hosts/{id}/deploy → 202 Accepted
      - bootc switch stages registry:5000/qarax-node-test:0.1.0 on the VM
      - systemctl reboot reboots the VM; Cloud Hypervisor resets the guest
      - VM boots the new ostree deployment; qarax-node starts with QARAX_NODE_VERSION=0.1.0
      - Host: installing → up
      - last_deployed_image is recorded
      - After re-init, node_version == "0.1.0"
    """
    client = Client(base_url=QARAX_URL, headers=AUTH_HEADERS)

    result = deploy_host.sync_detailed(
        host_id=upgrade_host_id,
        client=client,
        body=DeployHostRequest(
            image=NODE_IMAGE_V1,
            ssh_port=22,
            ssh_user="root",
            ssh_password="testpassword",
            install_bootc=False,  # real bootc is already present in the VM
            reboot=True,
        ),
    )
    assert result.status_code.value == 202, (
        f"Expected 202 Accepted, got {result.status_code}"
    )

    # Wait for the real reboot cycle to complete
    wait_for_host_status(client, upgrade_host_id, HostStatus.UP)

    host = get_host(client, upgrade_host_id)
    assert host.last_deployed_image == NODE_IMAGE_V1, (
        f"Expected last_deployed_image={NODE_IMAGE_V1!r}, got {host.last_deployed_image!r}"
    )

    # Re-init to capture fresh node_version from the restarted node
    init_host.sync_detailed(host_id=upgrade_host_id, client=client)
    host = get_host(client, upgrade_host_id)
    assert host.node_version == "0.1.0", (
        f"Expected node_version=0.1.0 after v1 deploy, got {host.node_version!r}"
    )
    assert not host.update_available, (
        "update_available should be False when node matches control plane version"
    )


def test_upgrade_uses_stored_credentials_and_updates_version(upgrade_host_id):
    """
    Full upgrade flow with real bootc switch + systemctl reboot:
      - Deploy v2 image to set last_deployed_image
      - Re-init: node_version = "0.2.0-test", update_available = True
      - POST /hosts/{id}/upgrade — no credentials in body (uses stored password)
      - Host: installing → up (real VM reboot occurs)
      - Re-init: node_version still "0.2.0-test" (same image re-deployed)
    """
    client = Client(base_url=QARAX_URL, headers=AUTH_HEADERS)

    # Deploy v2 to set last_deployed_image
    result = deploy_host.sync_detailed(
        host_id=upgrade_host_id,
        client=client,
        body=DeployHostRequest(
            image=NODE_IMAGE_V2,
            ssh_port=22,
            ssh_user="root",
            ssh_password="testpassword",
            install_bootc=False,
            reboot=True,
        ),
    )
    assert result.status_code.value == 202
    wait_for_host_status(client, upgrade_host_id, HostStatus.UP)

    # Init to capture the v2 node_version
    init_host.sync_detailed(host_id=upgrade_host_id, client=client)
    host = get_host(client, upgrade_host_id)
    assert host.node_version == EXPECTED_V2_VERSION, (
        f"Expected node_version={EXPECTED_V2_VERSION!r} after v2 deploy, "
        f"got {host.node_version!r}"
    )
    assert host.update_available, (
        "update_available should be True: node is 0.2.0-test, CP is 0.1.0"
    )
    assert host.last_deployed_image == NODE_IMAGE_V2

    # Upgrade — no explicit credentials; uses password stored in DB
    result = node_upgrade.sync_detailed(host_id=upgrade_host_id, client=client)
    assert result.status_code.value == 202, (
        f"Expected 202 Accepted from /upgrade, got {result.status_code}"
    )

    # Wait for the real reboot cycle to complete
    wait_for_host_status(client, upgrade_host_id, HostStatus.UP)

    # last_deployed_image unchanged (same image re-deployed)
    host = get_host(client, upgrade_host_id)
    assert host.last_deployed_image == NODE_IMAGE_V2

    # Re-init to capture version after upgrade
    init_host.sync_detailed(host_id=upgrade_host_id, client=client)
    host = get_host(client, upgrade_host_id)
    assert host.node_version == EXPECTED_V2_VERSION, (
        f"node_version should still be {EXPECTED_V2_VERSION!r} after upgrade, "
        f"got {host.node_version!r}"
    )
