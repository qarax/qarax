"""Shared helpers for qarax e2e tests."""

import asyncio
import os
import shlex
import subprocess
import time

from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.models import HostStatus, VmStatus

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
QARAX_TEST_TOKEN = os.getenv("QARAX_TEST_TOKEN", "e2e-test-token")
AUTH_HEADERS = {"Authorization": f"Bearer {QARAX_TEST_TOKEN}"}
VM_OPERATION_TIMEOUT = 30

COMPOSE_PROJECT = os.getenv("QARAX_COMPOSE_PROJECT", "e2e")
# Every qarax-node container of the compose project (qarax-node, qarax-node-2).
NODE_CONTAINER_PREFIX = f"{COMPOSE_PROJECT}-qarax-node"
# Upper bound for a single docker CLI call made by a test.
DOCKER_TIMEOUT = 120
# qarax-node --runtime-dir default: VM sockets, logs and cloud-init seed images.
NODE_RUNTIME_DIR = "/var/lib/qarax/vms"


async def call_api(endpoint_module, **kwargs):
    """Call a generated SDK endpoint and return its parsed body.

    Raises AssertionError with the HTTP status and body on any status >= 400.
    Use call_api_detailed when the test expects an error status.
    """
    response = await endpoint_module.asyncio_detailed(**kwargs)
    if response.status_code >= 400:
        raise AssertionError(
            f"{endpoint_module.__name__} failed with HTTP {response.status_code.value}: "
            f"{response.content.decode(errors='replace')}"
        )
    return response.parsed


async def call_api_detailed(endpoint_module, **kwargs):
    """Call a generated SDK endpoint and return the full response, whatever its status."""
    return await endpoint_module.asyncio_detailed(**kwargs)


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
    deadline = time.time() + timeout
    while time.time() < deadline:
        vm = await call_api(get_vm, client=client, vm_id=vm_id)
        if vm.status == expected_status:
            return vm
        await asyncio.sleep(0.5)

    vm = await call_api(get_vm, client=client, vm_id=vm_id)
    raise TimeoutError(
        f"VM {vm_id} did not reach {expected_status} within {timeout}s. Current: {vm.status}"
    )


async def wait_for_host_status(
    client,
    host_id,
    expected_status: HostStatus,
    timeout: int = 120,
):
    """Poll until a host reaches the expected status or the timeout expires."""
    deadline = time.time() + timeout
    while True:
        hosts = await call_api(list_hosts, client=client)
        host = next((h for h in hosts if h.id == host_id), None)
        assert host is not None, f"Host {host_id} is not registered"
        if host.status == expected_status:
            return host
        if time.time() >= deadline:
            raise TimeoutError(
                f"Host {host_id} did not reach {expected_status} within {timeout}s. "
                f"Current: {host.status}"
            )
        await asyncio.sleep(1)


def docker(*args, check=True) -> subprocess.CompletedProcess:
    """Run a docker CLI command with a deadline and return its captured output."""
    return subprocess.run(
        ["docker", *args],
        check=check,
        capture_output=True,
        text=True,
        timeout=DOCKER_TIMEOUT,
    )


def containers(prefix):
    """Names of the running containers whose name starts with prefix."""
    names = docker("ps", "--format", "{{.Names}}").stdout.splitlines()
    matching = [name for name in names if name.startswith(prefix)]
    assert matching, f"no running containers matching {prefix}"
    return matching


def compose_container(service):
    """ID of the running container of a compose service.

    The e2e hosts are registered with their compose service name as address.
    """
    container = docker(
        "ps",
        "-q",
        "--filter",
        f"label=com.docker.compose.project={COMPOSE_PROJECT}",
        "--filter",
        f"label=com.docker.compose.service={service}",
    ).stdout.strip()
    assert container, f"no running container for compose service {service}"
    return container


def path_exists_in_any(prefix, path):
    """True if `path` exists in any running container whose name starts with prefix."""
    return any(
        docker("exec", c, "test", "-e", path, check=False).returncode == 0
        for c in containers(prefix)
    )


def create_sparse_file_on_nodes(path, size_bytes):
    """Create a sparse file at `path` on every qarax-node container.

    Gives a test its own raw disk wherever the scheduler places its VM.
    """
    quoted = shlex.quote(path)
    command = f"mkdir -p $(dirname {quoted}) && truncate -s {size_bytes} {quoted}"
    for container in containers(NODE_CONTAINER_PREFIX):
        docker("exec", container, "sh", "-c", command)


def remove_file_from_nodes(path):
    """Remove `path` from every qarax-node container."""
    for container in containers(NODE_CONTAINER_PREFIX):
        docker("exec", container, "rm", "-f", path)
