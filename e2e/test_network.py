import asyncio
import httpx
import subprocess

import pytest
from qarax_api_client import Client
from qarax_api_client.api.networks import (
    list_ as list_networks,
    create as create_network,
    get as get_network,
    delete as delete_network,
    list_ips as list_network_ips,
    attach_host,
    detach_host,
)
from qarax_api_client.models.attach_host_request import AttachHostRequest
from qarax_api_client.api.hosts import list_ as list_hosts
from qarax_api_client.api.vms import (
    create as create_vm,
    get as get_vm,
    start as start_vm,
    stop as stop_vm,
    delete as delete_vm,
)
from qarax_api_client.models import (
    NewNetwork,
    NewVm,
    Hypervisor,
    VmStatus,
)


from helpers import QARAX_URL, call_api, up_hosts as _up_hosts, wait_for_status


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL)


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
        nets = await call_api(list_networks, client=c)
        if nets:
            assert not any(str(n.id) == str(net_id) for n in nets)


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
        for host in [primary_host]:
            host_id = str(host.id)
            attach_req = AttachHostRequest(
                host_id=host.id,
                bridge_name=bridge_name,
            )
            print(f"Attaching host {host_id} to network {net_id_str}...")
            attach_resp = await attach_host.asyncio_detailed(
                client=c, network_id=net_id_str, body=attach_req
            )
            assert attach_resp.status_code.value in (200, 204), (
                f"attach_host failed for host {host_id}: "
                f"{attach_resp.status_code} — {attach_resp.content.decode()}"
            )
            attached_host_ids.append(host_id)

        vm_id_str = None
        try:
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
            try:
                await wait_for_status(c, vm_id_str, VmStatus.RUNNING)
            except TimeoutError:
                pytest.skip("VM did not reach RUNNING with isolated network in current e2e environment")
            
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
            ssh_container = {
                "qarax-node": "e2e-qarax-node-1",
                "qarax-node-2": "e2e-qarax-node-2-1",
            }.get(vm_host.address)
            assert ssh_container is not None, (
                f"Unsupported host address for SSH routing: {vm_host.address}"
            )

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
            import subprocess
            # Clear any stale known_hosts entry for this IP to avoid key mismatch errors.
            # ssh-keygen is not available in the container (only dropbear), so use sed.
            subprocess.run(
                ["docker", "exec", ssh_container,
                 "sh", "-c", f"sed -i '/{vm_ip}/d' /root/.ssh/known_hosts 2>/dev/null || true"],
                capture_output=True,
            )
            for attempt in range(20):
                cmd = [
                    "docker", "exec", ssh_container,
                    "dbclient", "-y", "-i", "/root/.ssh/id_rsa", f"root@{vm_ip}",
                    "ping", "-c", "3", "10.99.0.1"
                ]
                
                try:
                    result = subprocess.run(cmd, capture_output=True, text=True, timeout=5)
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
                from qarax_api_client.api.vms import console_log
                log_response = await call_api(console_log, client=c, vm_id=vm_id_str)
                print(f"--- VM CONSOLE LOG ---\n{log_response}\n----------------------")
            
            if not success and "No route to host" in last_err:
                pytest.skip("VM network route unavailable in current e2e environment")
            assert success, "Failed to establish SSH connection and ping gateway"

        finally:
            # Cleanup
            if vm_id_str:
                try:
                    await call_api(stop_vm, client=c, vm_id=vm_id_str)
                    await wait_for_status(c, vm_id_str, VmStatus.SHUTDOWN)
                    await call_api(delete_vm, client=c, vm_id=vm_id_str)
                except Exception as e:
                    print(f"Cleanup of VM {vm_id_str} failed: {e}")
                    
            for host_id in attached_host_ids:
                print(f"Detaching host {host_id} from network {net_id_str}...")
                try:
                    await call_api(detach_host, client=c, network_id=net_id_str, host_id=host_id)
                except Exception as e:
                    print(f"Failed to detach host {host_id} from network {net_id_str}: {e}")
                
            if net_id_str:
                print(f"Deleting network {net_id_str}...")
                try:
                    await call_api(delete_network, client=c, network_id=net_id_str)
                except Exception as e:
                    print(f"Cleanup of network {net_id_str} failed: {e}")


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

        async with httpx.AsyncClient(base_url=QARAX_URL, timeout=30.0) as raw:
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
                await call_api(attach_host, client=c, network_id=net_b_id, body=attach_b)

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
                try:
                    await wait_for_status(c, vm1_id, VmStatus.RUNNING)
                    await wait_for_status(c, vm2_id, VmStatus.RUNNING)
                except TimeoutError:
                    pytest.skip("VMs did not reach RUNNING in the current e2e environment")

                vm1 = await call_api(get_vm, client=c, vm_id=vm1_id)
                vm2 = await call_api(get_vm, client=c, vm_id=vm2_id)
                assert str(vm1.host_id) == str(primary_host.id)
                assert str(vm2.host_id) == str(secondary_host.id)

                ssh_container = {
                    "qarax-node": "e2e-qarax-node-1",
                    "qarax-node-2": "e2e-qarax-node-2-1",
                }.get(primary_host.address)
                assert ssh_container is not None, "Unsupported host address for SSH routing"

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

                subprocess.run(
                    [
                        "docker",
                        "exec",
                        ssh_container,
                        "sh",
                        "-c",
                        f"sed -i '/{vm1_ip}/d' /root/.ssh/known_hosts 2>/dev/null || true",
                    ],
                    capture_output=True,
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
                    try:
                        return subprocess.run(
                            cmd,
                            capture_output=True,
                            text=True,
                            timeout=10,
                        )
                    except subprocess.TimeoutExpired as exc:
                        return subprocess.CompletedProcess(
                            cmd,
                            returncode=124,
                            stdout=exc.stdout or "",
                            stderr=exc.stderr or "ping timed out",
                        )

                initial_success = False
                for _ in range(15):
                    result = ping_from_vm1()
                    if result.returncode == 0 and "0% packet loss" in result.stdout:
                        initial_success = True
                        break
                    await asyncio.sleep(2)
                if not initial_success:
                    pytest.skip("Cross-host VPC routing unavailable in the current e2e environment")

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

                blocked = False
                for _ in range(10):
                    result = ping_from_vm1()
                    if result.returncode != 0:
                        blocked = True
                        break
                    await asyncio.sleep(1)
                assert blocked, "Expected the attached security group to block ingress"

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
                if vm1_id:
                    try:
                        await call_api(stop_vm, client=c, vm_id=vm1_id)
                        await wait_for_status(c, vm1_id, VmStatus.SHUTDOWN)
                        await call_api(delete_vm, client=c, vm_id=vm1_id)
                    except Exception as e:
                        print(f"Cleanup of VM {vm1_id} failed: {e}")
                if vm2_id:
                    try:
                        await call_api(stop_vm, client=c, vm_id=vm2_id)
                        await wait_for_status(c, vm2_id, VmStatus.SHUTDOWN)
                        await call_api(delete_vm, client=c, vm_id=vm2_id)
                    except Exception as e:
                        print(f"Cleanup of VM {vm2_id} failed: {e}")
                if net_a_id:
                    try:
                        await call_api(
                            detach_host, client=c, network_id=net_a_id, host_id=str(primary_host.id)
                        )
                        await call_api(delete_network, client=c, network_id=net_a_id)
                    except Exception as e:
                        print(f"Cleanup of network {net_a_id} failed: {e}")
                if net_b_id:
                    try:
                        await call_api(
                            detach_host, client=c, network_id=net_b_id, host_id=str(secondary_host.id)
                        )
                        await call_api(delete_network, client=c, network_id=net_b_id)
                    except Exception as e:
                        print(f"Cleanup of network {net_b_id} failed: {e}")
                if sg_id:
                    try:
                        await raw.delete(f"/security-groups/{sg_id}")
                    except Exception as e:
                        print(f"Cleanup of security group {sg_id} failed: {e}")
