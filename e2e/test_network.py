import asyncio
import subprocess

import httpx2
import pytest
from qarax_api_client import Client
from qarax_api_client.api.hosts import init as init_host
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.networks import (
    attach_host,
    detach_host,
)
from qarax_api_client.api.networks import (
    create as create_network,
)
from qarax_api_client.api.networks import (
    delete as delete_network,
)
from qarax_api_client.api.networks import (
    get as get_network,
)
from qarax_api_client.api.networks import (
    list_ as list_networks,
)
from qarax_api_client.api.networks import (
    list_hosts as list_network_hosts,
)
from qarax_api_client.api.networks import (
    list_ips as list_network_ips,
)
from qarax_api_client.api.vms import (
    console_log,
)
from qarax_api_client.api.vms import (
    create as create_vm,
)
from qarax_api_client.api.vms import (
    delete as delete_vm,
)
from qarax_api_client.api.vms import (
    get as get_vm,
)
from qarax_api_client.api.vms import (
    start as start_vm,
)
from qarax_api_client.models import (
    Hypervisor,
    NewNetwork,
    NewVm,
    VmStatus,
)
from qarax_api_client.models.attach_host_request import AttachHostRequest

from helpers import (
    AUTH_HEADERS,
    QARAX_URL,
    call_api,
    call_api_detailed,
    compose_container,
    docker,
    wait_for_status,
)
from helpers import up_hosts as _up_hosts


async def _attempt(label, step):
    """Await one cleanup step and return its failure instead of raising.

    Later steps still run; the test asserts on the collected failures.
    """
    try:
        await step
    except (AssertionError, httpx2.HTTPError) as exc:
        return [f"{label}: {exc}"]
    return []


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


@pytest.mark.asyncio
async def test_network_crud(client):
    """Test creating, listing, and deleting a network."""
    async with client as c:
        # Create network
        new_net = NewNetwork(
            name="e2e-net-crud",
            subnet="10.88.0.0/24",
            gateway="10.88.0.1",
            type_="isolated"
        )
        
        net_id = await call_api(create_network, client=c, body=new_net)
        assert net_id is not None
        
        try:
            # Get network
            net = await call_api(get_network, client=c, network_id=str(net_id))
            assert net.name == "e2e-net-crud"
            assert net.subnet == "10.88.0.0/24"
            
            # List networks
            nets = await call_api(list_networks, client=c)
            assert nets is not None
            assert any(n.id == net.id for n in nets)

        finally:
            # Delete network
            await call_api(delete_network, client=c, network_id=str(net_id))
            
        # Verify network is deleted
        resp = await call_api_detailed(get_network, client=c, network_id=str(net_id))
        assert resp.status_code == 404, (
            f"Expected deleted network to return 404, got {resp.status_code}: {resp.content}"
        )


@pytest.mark.asyncio
async def test_deleting_an_attached_network_removes_its_bridge(client):
    """Deleting a network detaches it from its hosts first, so no bridge,
    DHCP server or NAT rule is left behind on the node."""
    import uuid

    test_id = uuid.uuid4().hex[:6]
    bridge_name = f"br87{test_id}"
    async with client as c:
        hosts = _up_hosts(await call_api(list_hosts, client=c))
        assert hosts, "No UP hosts available"
        host = hosts[0]
        container = compose_container(str(host.address))
        net_id = await call_api(
            create_network,
            client=c,
            body=NewNetwork(
                name=f"e2e-net-del-{test_id}",
                subnet="10.87.0.0/24",
                gateway="10.87.0.1",
                type_="isolated",
            ),
        )
        deleted = False
        try:
            await call_api(
                attach_host,
                client=c,
                network_id=str(net_id),
                body=AttachHostRequest(host_id=host.id, bridge_name=bridge_name),
            )
            attached = await call_api(list_network_hosts, client=c, network_id=str(net_id))
            assert [(str(a.host_id), a.bridge_name) for a in attached] == [
                (str(host.id), bridge_name)
            ]
            assert docker("exec", container, "ip", "link", "show", bridge_name, check=False).returncode == 0

            await call_api(delete_network, client=c, network_id=str(net_id))
            deleted = True
            assert docker("exec", container, "ip", "link", "show", bridge_name, check=False).returncode != 0, (
                f"bridge {bridge_name} is still on {host.name} after the network was deleted"
            )
        finally:
            if not deleted:
                await call_api_detailed(delete_network, client=c, network_id=str(net_id))


@pytest.mark.asyncio
async def test_host_init_recreates_lost_network_bridges(client):
    """A reboot loses a node's bridges; host init re-creates the networks the
    host is attached to."""
    import uuid

    test_id = uuid.uuid4().hex[:6]
    bridge_name = f"br86{test_id}"
    async with client as c:
        hosts = _up_hosts(await call_api(list_hosts, client=c))
        assert hosts, "No UP hosts available"
        host = hosts[0]
        container = compose_container(str(host.address))
        net_id = await call_api(
            create_network,
            client=c,
            body=NewNetwork(
                name=f"e2e-net-reinit-{test_id}",
                subnet="10.86.0.0/24",
                gateway="10.86.0.1",
                type_="isolated",
            ),
        )
        try:
            await call_api(
                attach_host,
                client=c,
                network_id=str(net_id),
                body=AttachHostRequest(host_id=host.id, bridge_name=bridge_name),
            )
            docker("exec", container, "ip", "link", "delete", bridge_name)

            await call_api(init_host, client=c, host_id=str(host.id))

            # Init re-attaches networks in the background.
            for _ in range(60):
                if docker("exec", container, "ip", "link", "show", bridge_name, check=False).returncode == 0:
                    break
                await asyncio.sleep(0.5)
            else:
                pytest.fail(f"bridge {bridge_name} was not re-created on {host.name} by host init")
            addrs = docker("exec", container, "ip", "-4", "addr", "show", bridge_name).stdout
            assert "10.86.0.1/24" in addrs, addrs
        finally:
            await call_api_detailed(delete_network, client=c, network_id=str(net_id))


@pytest.mark.asyncio
async def test_vm_with_network(client):
    """Test creating a VM attached to a network, assigning IP, and checking connectivity via SSH."""
    async with client as c:
        # 1. Create Network
        import uuid
        test_id = uuid.uuid4().hex[:8]
        new_net = NewNetwork(
            name=f"e2e-net-{test_id}",
            subnet="10.99.0.0/24",
            gateway="10.99.0.1",
            type_="isolated"
        )
        net_id = await call_api(create_network, client=c, body=new_net)
        assert net_id is not None
        net_id_str = str(net_id)
        
        # Attach to one deterministic host to avoid cross-node network drift.
        hosts = _up_hosts(await call_api(list_hosts, client=c))
        assert hosts, "No UP hosts available"
        primary_host = next(
            (h for h in hosts if h.name == "local-node" or str(h.address) == "qarax-node"),
            hosts[0],
        )
        host_by_id = {str(primary_host.id): primary_host}
        attached_host_ids = []
        bridge_name = f"br99{test_id[:6]}"  # Linux iface names must be <= 15 chars
        vm_id_str = None
        try:
            for host in [primary_host]:
                host_id = str(host.id)
                attach_req = AttachHostRequest(
                    host_id=host.id,
                    bridge_name=bridge_name,
                )
                print(f"Attaching host {host_id} to network {net_id_str}...")
                await call_api(attach_host, client=c, network_id=net_id_str, body=attach_req)
                attached_host_ids.append(host_id)

            # 2. Create VM attached to network
            new_vm = NewVm(
                name=f"test-vm-{test_id}",
                hypervisor=Hypervisor.CLOUD_HV,
                boot_vcpus=1,
                max_vcpus=1,
                memory_size=256 * 1024 * 1024,
                network_id=net_id_str
            )

            vm_id = await call_api(create_vm, client=c, body=new_vm)
            assert vm_id is not None
            vm_id_str = str(vm_id)

            # 3. Start VM and wait for RUNNING status
            await call_api(start_vm, client=c, vm_id=vm_id_str)
            await wait_for_status(c, vm_id_str, VmStatus.RUNNING)

            # 4. Get VM IP address from API
            ips = await call_api(list_network_ips, client=c, network_id=net_id_str)
            vm_ip = None
            if ips:
                for alloc in ips:
                    if str(alloc.vm_id) == vm_id_str:
                        vm_ip = alloc.ip_address.split('/')[0]
                        break
                        
            assert vm_ip is not None, f"No IP allocated for VM {vm_id_str} on network {net_id_str}"

            vm = await call_api(get_vm, client=c, vm_id=vm_id_str)
            vm_host_id = str(vm.host_id)
            vm_host = host_by_id[vm_host_id]
            ssh_container = compose_container(vm_host.address)

            print(
                f"\nVM {vm_id_str} running on host {vm_host_id} "
                f"({vm_host.address}). Assigned IP: {vm_ip}"
            )

            # 5. Connect via SSH and check ping to gateway
            # Since the isolated network is created inside the qarax-node container (Linux bridge),
            # the host cannot route to 10.99.0.2 directly. Execute the SSH client from the
            # specific qarax-node container that the VM is running on.
            print(f"Connecting to VM via SSH from {ssh_container}...")
            success = False
            last_err = ""
            # Clear any stale known_hosts entry for this IP to avoid key mismatch errors.
            # ssh-keygen is not available in the container (only dropbear), so use sed.
            docker(
                "exec", ssh_container,
                "sh", "-c", f"sed -i '/{vm_ip}/d' /root/.ssh/known_hosts 2>/dev/null || true",
            )
            for attempt in range(20):
                cmd = [
                    "docker", "exec", ssh_container,
                    "dbclient", "-y", "-i", "/root/.ssh/id_rsa", f"root@{vm_ip}",
                    "ping", "-c", "3", "10.99.0.1"
                ]
                
                try:
                    result = await asyncio.to_thread(
                        subprocess.run,
                        cmd,
                        capture_output=True,
                        text=True,
                        timeout=5,
                        check=False,
                    )
                    if result.returncode == 0 and "0% packet loss" in result.stdout:
                        print(f"Ping output:\n{result.stdout}")
                        success = True
                        break
                    else:
                        err = result.stderr.strip() or result.stdout.strip()
                        last_err = err
                        print(f"SSH/Ping attempt {attempt+1} failed ({err}). Retrying in 2 seconds...")
                        await asyncio.sleep(2)
                except subprocess.TimeoutExpired:
                    last_err = "timeout"
                    print(f"SSH attempt {attempt+1} timed out. Retrying in 2 seconds...")
                    await asyncio.sleep(2)
            
            if not success:
                log_response = await call_api_detailed(
                    console_log, client=c, vm_id=vm_id_str
                )
                print(
                    f"--- VM CONSOLE LOG (HTTP {log_response.status_code.value}) ---\n"
                    f"{log_response.content.decode(errors='replace')}\n"
                    "----------------------"
                )
            assert success, f"Failed to establish SSH connection and ping gateway: {last_err}"

        finally:
            cleanup_errors = []
            if vm_id_str:
                cleanup_errors += await _attempt(
                    f"delete VM {vm_id_str}",
                    call_api(delete_vm, client=c, vm_id=vm_id_str),
                )
            for host_id in attached_host_ids:
                cleanup_errors += await _attempt(
                    f"detach host {host_id} from network {net_id_str}",
                    call_api(detach_host, client=c, network_id=net_id_str, host_id=host_id),
                )
            if net_id_str:
                cleanup_errors += await _attempt(
                    f"delete network {net_id_str}",
                    call_api(delete_network, client=c, network_id=net_id_str),
                )
            assert not cleanup_errors, f"Cleanup failed: {cleanup_errors}"


@pytest.mark.asyncio
async def test_vpc_routing_and_security_group_updates(client):
    async with client as c:
        import uuid

        test_id = uuid.uuid4().hex[:8]
        vpc_name = f"e2e-vpc-{test_id}"
        subnet_a = "10.121.1.0/24"
        gateway_a = "10.121.1.1"
        subnet_b = "10.121.2.0/24"
        gateway_b = "10.121.2.1"

        hosts = _up_hosts(await call_api(list_hosts, client=c))
        assert hosts, "No UP hosts available"
        primary_host = next(
            (h for h in hosts if h.name == "local-node" or str(h.address) == "qarax-node"),
            hosts[0],
        )
        secondary_host = next((h for h in hosts if h.id != primary_host.id), None)
        if secondary_host is None:
            pytest.skip("Cross-host VPC test requires at least two UP hosts")

        vm1_id = None
        vm2_id = None
        sg_id = None
        net_a_id = None
        net_b_id = None
        attached = []

        async with httpx.AsyncClient(base_url=QARAX_URL, timeout=30.0, headers=AUTH_HEADERS) as raw:
            try:
                net_a_resp = await raw.post(
                    "/networks",
                    json={
                        "name": f"e2e-vpc-a-{test_id}",
                        "subnet": subnet_a,
                        "gateway": gateway_a,
                        "vpc_name": vpc_name,
                        "type": "isolated",
                    },
                )
                assert net_a_resp.status_code == 201, net_a_resp.text
                net_a_id = net_a_resp.text

                net_b_resp = await raw.post(
                    "/networks",
                    json={
                        "name": f"e2e-vpc-b-{test_id}",
                        "subnet": subnet_b,
                        "gateway": gateway_b,
                        "vpc_name": vpc_name,
                        "type": "isolated",
                    },
                )
                assert net_b_resp.status_code == 201, net_b_resp.text
                net_b_id = net_b_resp.text

                attach_a = AttachHostRequest(
                    host_id=primary_host.id,
                    bridge_name=f"qvpa{test_id[:6]}",
                )
                attach_b = AttachHostRequest(
                    host_id=secondary_host.id,
                    bridge_name=f"qvpb{test_id[:6]}",
                )
                await call_api(attach_host, client=c, network_id=net_a_id, body=attach_a)
                attached.append((net_a_id, primary_host.id))
                await call_api(attach_host, client=c, network_id=net_b_id, body=attach_b)
                attached.append((net_b_id, secondary_host.id))

                vm1_resp = await raw.post(
                    "/vms",
                    json={
                        "name": f"e2e-vpc-vm1-{test_id}",
                        "hypervisor": "cloud_hv",
                        "boot_vcpus": 1,
                        "max_vcpus": 1,
                        "memory_size": 256 * 1024 * 1024,
                        "network_id": net_a_id,
                        "config": {},
                    },
                )
                assert vm1_resp.status_code == 201, vm1_resp.text
                vm1_id = str(vm1_resp.json())

                vm2_resp = await raw.post(
                    "/vms",
                    json={
                        "name": f"e2e-vpc-vm2-{test_id}",
                        "hypervisor": "cloud_hv",
                        "boot_vcpus": 1,
                        "max_vcpus": 1,
                        "memory_size": 256 * 1024 * 1024,
                        "network_id": net_b_id,
                        "config": {},
                    },
                )
                assert vm2_resp.status_code == 201, vm2_resp.text
                vm2_id = str(vm2_resp.json())

                await call_api(start_vm, client=c, vm_id=vm1_id)
                await call_api(start_vm, client=c, vm_id=vm2_id)
                await wait_for_status(c, vm1_id, VmStatus.RUNNING)
                await wait_for_status(c, vm2_id, VmStatus.RUNNING)

                vm1 = await call_api(get_vm, client=c, vm_id=vm1_id)
                vm2 = await call_api(get_vm, client=c, vm_id=vm2_id)
                assert str(vm1.host_id) == str(primary_host.id)
                assert str(vm2.host_id) == str(secondary_host.id)

                ssh_container = compose_container(primary_host.address)

                ips_a = await call_api(list_network_ips, client=c, network_id=net_a_id)
                ips_b = await call_api(list_network_ips, client=c, network_id=net_b_id)
                vm1_ip = next(
                    alloc.ip_address.split("/")[0]
                    for alloc in ips_a
                    if str(alloc.vm_id) == vm1_id
                )
                vm2_ip = next(
                    alloc.ip_address.split("/")[0]
                    for alloc in ips_b
                    if str(alloc.vm_id) == vm2_id
                )

                docker(
                    "exec",
                    ssh_container,
                    "sh",
                    "-c",
                    f"sed -i '/{vm1_ip}/d' /root/.ssh/known_hosts 2>/dev/null || true",
                )

                def ping_from_vm1():
                    cmd = [
                        "docker",
                        "exec",
                        ssh_container,
                        "dbclient",
                        "-y",
                        "-i",
                        "/root/.ssh/id_rsa",
                        f"root@{vm1_ip}",
                        "ping",
                        "-c",
                        "3",
                        vm2_ip,
                    ]
                    # busybox ping waits 10s for a reply after its last probe,
                    # so an unanswered ping needs ~12s to print its statistics.
                    try:
                        return subprocess.run(
                            cmd,
                            capture_output=True,
                            text=True,
                            timeout=20,
                            check=False,
                        )
                    except subprocess.TimeoutExpired as exc:
                        # TimeoutExpired carries raw bytes even with text=True.
                        return subprocess.CompletedProcess(
                            cmd,
                            returncode=124,
                            stdout=(exc.stdout or b"").decode(errors="replace"),
                            stderr=(exc.stderr or b"ping timed out").decode(errors="replace"),
                        )

                initial_success = False
                for _ in range(15):
                    result = ping_from_vm1()
                    if result.returncode == 0 and "0% packet loss" in result.stdout:
                        initial_success = True
                        break
                    await asyncio.sleep(2)
                assert initial_success, (
                    "Expected cross-host VPC routing from vm1 to vm2 before any security "
                    f"group: rc={result.returncode} stdout={result.stdout!r} "
                    f"stderr={result.stderr!r}"
                )

                sg_resp = await raw.post(
                    "/security-groups",
                    json={
                        "name": f"e2e-sg-{test_id}",
                        "description": "deny ingress until allowed",
                    },
                )
                assert sg_resp.status_code == 201, sg_resp.text
                sg_id = sg_resp.text

                attach_resp = await raw.post(
                    f"/vms/{vm2_id}/security-groups",
                    json={"security_group_id": sg_id},
                )
                assert attach_resp.status_code == 204, attach_resp.text

                # Blocked means the SSH session to vm1 worked and ping itself
                # reported every probe lost, not that anything exited non-zero.
                blocked = False
                for _ in range(10):
                    result = ping_from_vm1()
                    if "100% packet loss" in result.stdout:
                        blocked = True
                        break
                    await asyncio.sleep(1)
                assert blocked, (
                    "Expected the attached security group to block ingress: "
                    f"rc={result.returncode} stdout={result.stdout!r} stderr={result.stderr!r}"
                )

                rule_resp = await raw.post(
                    f"/security-groups/{sg_id}/rules",
                    json={
                        "direction": "ingress",
                        "protocol": "icmp",
                        "cidr": subnet_a,
                    },
                )
                assert rule_resp.status_code == 201, rule_resp.text

                allowed = False
                for _ in range(15):
                    result = ping_from_vm1()
                    if result.returncode == 0 and "0% packet loss" in result.stdout:
                        allowed = True
                        break
                    await asyncio.sleep(2)
                assert allowed, "Expected the ICMP ingress rule to restore connectivity"
            finally:
                cleanup_errors = []
                for vm_id in (vm1_id, vm2_id):
                    if vm_id:
                        cleanup_errors += await _attempt(
                            f"delete VM {vm_id}", call_api(delete_vm, client=c, vm_id=vm_id)
                        )
                for net_id, host_id in attached:
                    cleanup_errors += await _attempt(
                        f"detach host {host_id} from network {net_id}",
                        call_api(detach_host, client=c, network_id=net_id, host_id=str(host_id)),
                    )
                for net_id in (net_a_id, net_b_id):
                    if net_id:
                        cleanup_errors += await _attempt(
                            f"delete network {net_id}",
                            call_api(delete_network, client=c, network_id=net_id),
                        )
                if sg_id:
                    resp = await raw.delete(f"/security-groups/{sg_id}")
                    if resp.status_code != 204:
                        cleanup_errors.append(
                            f"delete security group {sg_id}: HTTP {resp.status_code} {resp.text}"
                        )
                assert not cleanup_errors, f"Cleanup failed: {cleanup_errors}"
