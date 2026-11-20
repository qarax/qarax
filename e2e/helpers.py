"""Shared helpers for qarax e2e tests."""

import asyncio
import os
import shlex
import subprocess
import time

from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.storage_pools import attach_host as attach_pool_host
from qarax_api_client.api.jobs import get as get_job
from qarax_api_client.api.vms import exec_ as exec_vm
from qarax_api_client.api.vms import get as get_vm
from qarax_api_client.models import ExecVmRequest, HostStatus, VmStatus
from qarax_api_client.models.attach_pool_host_request import AttachPoolHostRequest

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
# The test registry as seen from the test runner (host-side port mapping), and
# from inside the docker network.
REGISTRY_PUSH_URL = os.getenv("REGISTRY_PUSH_URL", "localhost:5001")
REGISTRY_INTERNAL_URL = os.getenv("REGISTRY_INTERNAL_URL", "registry:5000")


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


async def exec_when_ready(client, vm_id, script):
    """Run a shell script in the guest once its agent answers."""
    request = ExecVmRequest(command=["/bin/sh", "-c", script], timeout_secs=20)
    deadline = time.monotonic() + 60
    while True:
        response = await call_api_detailed(exec_vm, client=client, vm_id=vm_id, body=request)
        if response.status_code == 200:
            return response.parsed
        # 422 until the guest agent is up.
        assert response.status_code == 422, response.content.decode()
        assert time.monotonic() < deadline, "the guest agent never answered"
        await asyncio.sleep(1)


async def attach_overlaybd_pool(c, hosts, pool_id):
    """Attach an OverlayBD pool to every UP host whose node accepts it.

    A node refuses OverlayBD without an overlaybd-tcmu daemon, which the
    compose stack runs on qarax-node only (two daemons can't share a kernel's
    TCMU handler). The scheduler then places image VMs on a host that has it.
    """
    accepted = []
    for host in up_hosts(hosts):
        resp = await call_api_detailed(
            attach_pool_host,
            client=c,
            pool_id=pool_id,
            body=AttachPoolHostRequest(host_id=host.id),
        )
        if resp.status_code == 204:
            accepted.append(host.name)
        else:
            assert resp.status_code == 422 and b"overlaybd-tcmu" in resp.content, (
                f"attaching OverlayBD pool to {host.name}: {resp.status_code} {resp.content}"
            )
    assert accepted, "no host accepted the OverlayBD pool"


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


async def wait_for_start(
    client, vm_id, start_job_id, timeout: int = VM_OPERATION_TIMEOUT
):
    """Wait for a started VM to reach RUNNING; on timeout, say how its start job ended."""
    try:
        return await wait_for_status(client, vm_id, VmStatus.RUNNING, timeout)
    except TimeoutError as e:
        job = await call_api(get_job, client=client, job_id=start_job_id)
        raise TimeoutError(f"{e}. Start job {job.status}: {job.error}") from e


async def wait_for_host_status(
    client,
    host_id,
    expected_status: HostStatus,
    timeout: int = 120,
):
    """Poll until a host reaches the expected status or the timeout expires."""
    deadline = time.time() + timeout
    while True:
        hosts = (await call_api(list_hosts, client=client)).items
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


def vm_diagnostics(vm_id, max_bytes=6000):
    """What a node knows of a VM, for a failure message: the end of its serial
    console (Cloud Hypervisor writes <vm>.console.log, Firecracker <vm>.fc.log)
    and the node's log lines that mention it."""
    for container in containers(NODE_CONTAINER_PREFIX):
        for name in (f"{vm_id}.console.log", f"{vm_id}.fc.log"):
            console = docker(
                "exec", container, "tail", "-c", str(max_bytes), f"/var/lib/qarax/vms/{name}",
                check=False,
            )
            if console.returncode == 0:
                logs = docker("logs", container, check=False)
                mentions = [
                    line
                    for line in (logs.stdout + logs.stderr).splitlines()
                    if str(vm_id) in line
                ]
                return (
                    f"--- {name} on {container} ---\n{console.stdout}\n"
                    f"--- {container} log lines for the VM ---\n" + "\n".join(mentions[-40:])
                )
    return f"(no console log for VM {vm_id} on any node)"


def node_storage_diagnostics(max_lines=80):
    """The state of OverlayBD on every node, for a failure message: whether
    overlaybd-tcmu runs, the end of its log, the kernel's, and the SCSI disks."""
    script = (
        "pgrep -a overlaybd-tcmu || echo 'overlaybd-tcmu is not running'; "
        f"echo '--- overlaybd.log'; tail -n {max_lines} /var/log/overlaybd.log 2>&1; "
        f"echo '--- dmesg'; dmesg 2>&1 | tail -n {max_lines}; "
        "echo '--- disks'; ls -l /sys/block | grep ' sd' || true"
    )
    return "\n".join(
        f"=== {container} ===\n" + docker("exec", container, "sh", "-c", script, check=False).stdout
        for container in containers(NODE_CONTAINER_PREFIX)
    )


def overlaybd_tcmu_running():
    """Whether overlaybd-tcmu runs on the node that has it (the compose stack
    runs it on qarax-node only)."""
    return any(
        docker("exec", container, "pgrep", "-x", "overlaybd-tcmu", check=False).returncode == 0
        for container in containers(NODE_CONTAINER_PREFIX)
    )


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


def create_image_on_nodes(path, size_bytes, image_format):
    """Create an empty disk image (`raw` or `qcow2`) at `path` on every
    qarax-node container."""
    quoted = shlex.quote(path)
    command = (
        f"mkdir -p $(dirname {quoted}) && "
        f"qemu-img create -q -f {shlex.quote(image_format)} {quoted} {size_bytes}"
    )
    for container in containers(NODE_CONTAINER_PREFIX):
        docker("exec", container, "sh", "-c", command)


def remove_file_from_nodes(path):
    """Remove `path` from every qarax-node container."""
    for container in containers(NODE_CONTAINER_PREFIX):
        docker("exec", container, "rm", "-f", path)
