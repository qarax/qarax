"""Guest networking on one node: DHCP, DNS, and filtering between VMs on the
same network.

The guests boot the test initramfs; commands run in them over SSH from the
node that hosts them (guest-ssh), since guest subnets are only routable there.
"""

import asyncio
import re
import subprocess
import time
import uuid

import httpx
import pytest

from helpers import AUTH_HEADERS, QARAX_URL, compose_container

GUEST_READY_TIMEOUT = 60


def _result(cmd, output, returncode):
    return subprocess.CompletedProcess(cmd, returncode, stdout=output, stderr="")


class GuestNetwork:
    """An isolated network on one UP host, with the VMs and security groups a
    test makes on it. Everything is deleted when the test ends."""

    def __init__(self, api, subnet, gateway):
        self.api = api
        self.subnet = subnet
        self.gateway = gateway
        self.name = f"e2e-guestnet-{uuid.uuid4().hex[:8]}"
        self.network_id = None
        self.host_id = None
        self.node = None
        self.vm_ids = []
        self.sg_ids = []

    async def create(self):
        hosts = (await self.api.get("/hosts")).json()
        host = next(
            (
                h
                for h in hosts
                if h["status"] == "up" and h["address"].startswith("qarax-node")
            ),
            None,
        )
        assert host, "needs an UP qarax-node host"
        self.host_id = host["id"]
        self.node = compose_container(host["address"])

        resp = await self.api.post(
            "/networks",
            json={
                "name": self.name,
                "subnet": self.subnet,
                "gateway": self.gateway,
                "type": "isolated",
            },
        )
        assert resp.status_code == 201, resp.text
        self.network_id = resp.text.strip('"')
        resp = await self.api.post(
            f"/networks/{self.network_id}/hosts",
            json={"host_id": self.host_id, "bridge_name": f"qgn{uuid.uuid4().hex[:8]}"},
        )
        assert resp.status_code in (200, 201, 204), resp.text

    async def vm(self, ip=None):
        """Create and start a VM with a NIC on the network; return its address."""
        # A NIC with a requested address, or the network's next free one.
        if ip:
            nics = {
                "networks": [{"id": "net0", "network_id": self.network_id, "ip": ip}]
            }
        else:
            nics = {"network_id": self.network_id}
        resp = await self.api.post(
            "/vms",
            json={
                "name": f"{self.name}-vm{len(self.vm_ids)}",
                "hypervisor": "cloud_hv",
                "boot_vcpus": 1,
                "max_vcpus": 1,
                "memory_size": 256 * 1024 * 1024,
                **nics,
            },
        )
        assert resp.status_code == 201, resp.text
        vm_id = str(resp.json())
        self.vm_ids.append(vm_id)

        resp = await self.api.post(f"/vms/{vm_id}/start")
        assert resp.status_code in (200, 202), resp.text
        deadline = time.monotonic() + GUEST_READY_TIMEOUT
        while (await self.api.get(f"/vms/{vm_id}")).json()["status"] != "running":
            assert time.monotonic() < deadline, f"VM {vm_id} did not reach running"
            await asyncio.sleep(1)

        nics = (await self.api.get(f"/vms/{vm_id}/nics")).json()
        address = nics[0]["ip_address"].split("/")[0]
        await self.wait_for_ssh(address)
        return vm_id, address

    def guest(self, address, *cmd, timeout=25):
        """Run cmd in the guest at address; never raises on a timeout."""
        argv = ["docker", "exec", self.node, "guest-ssh", f"root@{address}", *cmd]
        try:
            return subprocess.run(
                argv, capture_output=True, text=True, timeout=timeout, check=False
            )
        except subprocess.TimeoutExpired as exc:
            return _result(argv, (exc.stdout or b"").decode(errors="replace"), 124)

    async def wait_for_ssh(self, address):
        deadline = time.monotonic() + GUEST_READY_TIMEOUT
        while self.guest(address, "true", timeout=10).returncode != 0:
            assert time.monotonic() < deadline, f"no SSH to the guest at {address}"
            await asyncio.sleep(1)

    def ping_loss(self, src, dst, *args):
        """Packet loss (%) of a ping from the guest at src to dst; None when
        the ping didn't run (no SSH)."""
        result = self.guest(src, "ping", "-c", "3", "-W", "1", *args, dst)
        match = re.search(r"(\d+)% packet loss", result.stdout)
        return int(match.group(1)) if match else None

    async def expect_ping_loss(self, src, dst, loss, *args, attempts=10):
        """Poll until a ping from src to dst loses `loss` percent; rules take
        effect within a moment of the API call."""
        got = None
        for _ in range(attempts):
            got = self.ping_loss(src, dst, *args)
            if got == loss:
                return
            await asyncio.sleep(1)
        pytest.fail(
            f"ping {src} -> {dst} {args}: expected {loss}% loss, last got {got}"
        )

    async def security_group(self, vm_id, rules=()):
        resp = await self.api.post(
            "/security-groups", json={"name": f"{self.name}-sg{len(self.sg_ids)}"}
        )
        assert resp.status_code == 201, resp.text
        sg_id = resp.text.strip('"')
        self.sg_ids.append(sg_id)
        for rule in rules:
            await self.add_rule(sg_id, rule)
        resp = await self.api.post(
            f"/vms/{vm_id}/security-groups", json={"security_group_id": sg_id}
        )
        assert resp.status_code == 204, resp.text
        return sg_id

    async def add_rule(self, sg_id, rule):
        resp = await self.api.post(f"/security-groups/{sg_id}/rules", json=rule)
        assert resp.status_code == 201, resp.text

    async def delete(self):
        failures = []
        for path in [f"/vms/{vm_id}" for vm_id in self.vm_ids] + [
            f"/security-groups/{sg_id}" for sg_id in self.sg_ids
        ]:
            resp = await self.api.delete(path)
            if resp.status_code not in (200, 202, 204, 404):
                failures.append(f"DELETE {path}: {resp.status_code} {resp.text}")
        if self.network_id:
            for path in (
                f"/networks/{self.network_id}/hosts/{self.host_id}",
                f"/networks/{self.network_id}",
            ):
                resp = await self.api.delete(path)
                if resp.status_code not in (200, 204, 404):
                    failures.append(f"DELETE {path}: {resp.status_code} {resp.text}")
        assert not failures, failures


@pytest.fixture
async def api():
    async with httpx.AsyncClient(
        base_url=QARAX_URL, headers=AUTH_HEADERS, timeout=60
    ) as client:
        yield client


@pytest.fixture
async def guest_network(api):
    """Factory for GuestNetwork, cleaned up after the test."""
    made = []

    async def make(subnet, gateway):
        network = GuestNetwork(api, subnet, gateway)
        made.append(network)
        await network.create()
        return network

    yield make
    for network in made:
        await network.delete()


@pytest.mark.asyncio
async def test_dhcp_answers_each_nic_with_its_ipam_address_and_mask(guest_network):
    # A /23 and an address that isn't the first free one: a server handing
    # out leases from a pool, or assuming /24, gets both wrong.
    net = await guest_network("10.141.0.0/23", "10.141.0.1")
    _, address = await net.vm(ip="10.141.1.77")

    result = net.guest(
        address,
        "udhcpc",
        "-i",
        "eth0",
        "-f",
        "-q",
        "-n",
        "-t",
        "5",
        "-T",
        "1",
        "-s",
        "/etc/udhcpc.script",
    )
    assert (
        "lease ip=10.141.1.77 subnet=255.255.254.0 router=10.141.0.1 dns=10.141.0.1"
        in (result.stdout)
    ), f"rc={result.returncode} stdout={result.stdout!r} stderr={result.stderr!r}"


@pytest.mark.asyncio
async def test_guests_resolve_names_through_their_gateway(guest_network):
    net = await guest_network("10.142.0.0/24", "10.142.0.1")
    _, address = await net.vm()

    # "registry" is a compose service: only the node's own resolver knows it.
    result = net.guest(address, "nslookup", "registry", "10.142.0.1")
    assert re.search(
        r"Name:\s+registry\s+Address:\s+\d+\.\d+\.\d+\.\d+", result.stdout
    ), f"rc={result.returncode} stdout={result.stdout!r} stderr={result.stderr!r}"


@pytest.mark.asyncio
async def test_security_groups_filter_traffic_between_vms_on_one_network(guest_network):
    net = await guest_network("10.143.0.0/24", "10.143.0.1")
    _, a = await net.vm()
    b_id, b = await net.vm()
    await net.expect_ping_loss(a, b, 0)

    # Default deny ingress, also from a VM on the same bridge.
    sg_id = await net.security_group(b_id)
    await net.expect_ping_loss(a, b, 100)
    # b's own connections still work: replies come back as established.
    await net.expect_ping_loss(b, a, 0)

    await net.add_rule(
        sg_id, {"direction": "ingress", "protocol": "icmp", "cidr": "10.143.0.0/24"}
    )
    await net.expect_ping_loss(a, b, 0)


@pytest.mark.asyncio
async def test_a_vm_cannot_send_from_an_address_it_was_not_given(guest_network):
    net = await guest_network("10.144.0.0/24", "10.144.0.1")
    _, a = await net.vm()
    _, b = await net.vm()

    added = net.guest(a, "ip", "addr", "add", "10.144.0.200/24", "dev", "eth0")
    assert added.returncode == 0, added.stderr
    await net.expect_ping_loss(a, b, 100, "-I", "10.144.0.200")
    await net.expect_ping_loss(a, b, 0)


@pytest.mark.asyncio
async def test_security_groups_see_through_the_hosts_masquerade(guest_network):
    # Networks without a VPC route to each other through the host, which
    # masquerades a's traffic to b's gateway address. b's security group must
    # still deny it (it isn't the host's own traffic) and match a's subnet.
    net_a = await guest_network("10.145.0.0/24", "10.145.0.1")
    net_b = await guest_network("10.146.0.0/24", "10.146.0.1")
    _, a = await net_a.vm()
    b_id, b = await net_b.vm()
    await net_a.expect_ping_loss(a, b, 0)

    sg_id = await net_b.security_group(b_id)
    await net_a.expect_ping_loss(a, b, 100)

    await net_b.add_rule(
        sg_id, {"direction": "ingress", "protocol": "icmp", "cidr": "10.145.0.0/24"}
    )
    await net_a.expect_ping_loss(a, b, 0)
