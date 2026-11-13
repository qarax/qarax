"""
E2E tests for cloud-init NoCloud seed disk support.

A VM created with cloud-init data boots with a NoCloud seed image that the
node writes next to the VM's runtime files, and the seed image is removed
when the VM is deleted.
"""

import pytest
from qarax_api_client import Client
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
from qarax_api_client.api.vms import (
    stop as stop_vm,
)
from qarax_api_client.models import Hypervisor, NewVm, VmStatus

from helpers import (
    AUTH_HEADERS,
    NODE_CONTAINER_PREFIX,
    NODE_RUNTIME_DIR,
    QARAX_URL,
    call_api,
    path_exists_in_any,
    wait_for_status,
)

MINIMAL_USER_DATA = """\
#cloud-config
# Minimal cloud-init user-data for e2e testing.
# The test kernel/initramfs may not have cloud-init installed;
# the important thing is that the VM boots with the cidata disk present.
runcmd:
  - echo "cloud-init e2e test" > /tmp/cloud-init-ran
"""

MINIMAL_META_DATA = """\
instance-id: e2e-cloud-init-test
local-hostname: e2e-cloud-init-vm
"""


@pytest.fixture
def client():
    return Client(base_url=QARAX_URL, headers=AUTH_HEADERS)


@pytest.mark.asyncio
async def test_cloud_init_vm_boots(client):
    """VM with cloud-init data boots with a seed image that is removed with the VM."""
    async with client as c:
        new_vm = NewVm(
            name="e2e-cloud-init-boot",
            hypervisor=Hypervisor.CLOUD_HV,
            boot_vcpus=1,
            max_vcpus=1,
            memory_size=256 * 1024 * 1024,
            cloud_init_user_data=MINIMAL_USER_DATA,
            cloud_init_meta_data=MINIMAL_META_DATA,
        )
        vm_id = await call_api(create_vm, client=c, body=new_vm)
        assert vm_id is not None
        vm_id_str = str(vm_id).strip('"')
        # qarax-node/src/cloud_hypervisor/manager/helpers.rs: cloud_init_seed_path
        seed_path = f"{NODE_RUNTIME_DIR}/{vm_id_str}-cidata.img"

        try:
            await call_api(start_vm, client=c, vm_id=vm_id_str)
            await wait_for_status(c, vm_id_str, VmStatus.RUNNING)
            assert path_exists_in_any(NODE_CONTAINER_PREFIX, seed_path), (
                f"cloud-init seed image {seed_path} not found on any node"
            )

            await call_api(stop_vm, client=c, vm_id=vm_id_str)
            vm = await call_api(get_vm, client=c, vm_id=vm_id_str)
            assert vm.status == VmStatus.SHUTDOWN
        finally:
            await call_api(delete_vm, client=c, vm_id=vm_id_str)

        assert not path_exists_in_any(NODE_CONTAINER_PREFIX, seed_path), (
            f"cloud-init seed image {seed_path} should be removed with the VM"
        )
