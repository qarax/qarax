import asyncio
import os
import time

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


QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")
VM_OPERATION_TIMEOUT = 30


@pytest.fixture
def client():
    """Create a qarax API client."""
    return Client(base_url=QARAX_URL)


async def call_api(endpoint_module, **kwargs):
    asyncio_fn = getattr(endpoint_module, "asyncio", None)
    if callable(asyncio_fn):
        return await asyncio_fn(**kwargs)

    detailed_fn = getattr(endpoint_module, "asyncio_detailed", None)
    if callable(detailed_fn):
        response = await detailed_fn(**kwargs)
        return response.parsed

    raise AttributeError(f"{endpoint_module.__name__} has no async entrypoint")


async def wait_for_status(client, vm_id: str, expected_status: VmStatus, timeout: int = VM_OPERATION_TIMEOUT):
    start_time = time.time()
    while time.time() - start_time < timeout:
        vm = await call_api(get_vm, client=client, vm_id=vm_id)
        if vm.status == expected_status:
            return vm
        await asyncio.sleep(0.5)

    vm = await call_api(get_vm, client=client, vm_id=vm_id)
    raise TimeoutError(f"VM {vm_id} did not reach status {expected_status}. Current: {vm.status}")


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
        
        # Attach the network to every available host so dynamic scheduling can
        # place the VM on either node without breaking guest connectivity.
        hosts = await call_api(list_hosts, client=c)
        assert hosts, "No hosts available"
        host_by_id = {str(host.id): host for host in hosts}
        attached_host_ids = []
        for host in hosts:
            host_id = str(host.id)
            attach_req = AttachHostRequest(
                host_id=host.id,
                bridge_name="br-10-99",
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
                        print(f"SSH/Ping attempt {attempt+1} failed ({err}). Retrying in 2 seconds...")
                        await asyncio.sleep(2)
                except subprocess.TimeoutExpired:
                    print(f"SSH attempt {attempt+1} timed out. Retrying in 2 seconds...")
                    await asyncio.sleep(2)
            
            if not success:
                from qarax_api_client.api.vms import console_log
                log_response = await call_api(console_log, client=c, vm_id=vm_id_str)
                print(f"--- VM CONSOLE LOG ---\n{log_response}\n----------------------")
            
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
