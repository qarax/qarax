import functools
import logging
import os

import pytest
import qarax
from qarax.api import hosts_api
from qarax.api import storage_api
from qarax.model.host import Host
from qarax.model.storage import Storage
from qarax.model.storage_config import StorageConfig

import terraform
import util

log = logging.getLogger(__name__)


@pytest.fixture(scope="module", autouse=True)
def tf():
    tf = terraform.Terraform(
        workdir=os.path.join(os.path.abspath(os.path.dirname(__file__)), "terraform")
    )

    yield tf


@pytest.fixture(scope="module", autouse=True)
def vm(tf):
    _, err = tf.init()
    if err:
        log.error(err)
        raise Exception("Failed to init terraform plan")

    _, err = tf.apply()
    if err:
        log.error(err)
        raise Exception("Failed to apply terraform plan")

    vm_json, err = tf.show()
    if err:
        log.error(err)
        raise Exception("Failed to show terraform resource")

    yield vm_json


@pytest.fixture(scope="module", autouse=True)
def vm_ip(vm):
    log.info("Getting VM IP address")
    vm_ip = None

    for resource in vm["values"]["root_module"]["resources"]:
        if "network_interface" in resource["values"]:
            vm_ip = resource["values"]["network_interface"][0]["addresses"][0]

    if vm_ip is None:
        raise Exception("VM IP address not found")

    log.info("VM IP address %s", vm_ip)
    yield vm_ip


@pytest.fixture(scope="module", autouse=True)
def host_config(vm_ip):
    import time

    host_config = {
        "name": "e2e-test-host" + str(time.time()),
        "address": vm_ip,
        "username": "root",
        "password": "centos",
        "port": 50051,
    }

    yield host_config


@pytest.fixture(scope="module", autouse=True)
def vm_drives(vm_ip, host_config):
    kernel = "kernels/vmlinux.bin"
    rootfs = "rootfs/bionic.rootfs.ext4"
    base_url = "https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/"

    util.run_parallel_jobs(
        [
            functools.partial(
                util.download_file_to_host,
                url=base_url + kernel,
                host=vm_ip,
                username=host_config["username"],
                password=host_config["password"],
                dest_path=f'/root/{kernel.split("/")[1]}',
            ),
            functools.partial(
                util.download_file_to_host,
                url=base_url + rootfs,
                host=vm_ip,
                username=host_config["username"],
                password=host_config["password"],
                dest_path=f'/root/{rootfs.split("/")[1]}',
            ),
        ]
    )


@pytest.fixture(scope="module", autouse=True)
def qarax_configuration():
    configuration = qarax.Configuration(host="http://localhost:3000")

    yield configuration


@pytest.fixture(scope="module", autouse=True)
def api_client(qarax_configuration):
    api_client = qarax.ApiClient(qarax_configuration)

    yield api_client


@pytest.mark.order(1)
def test_install_host(api_client, vm_ip, host_config):
    api_instance = hosts_api.HostsApi(api_client)
    host = Host(
        name=host_config["name"],
        address=host_config["address"],
        host_user=host_config["username"],
        password=host_config["password"],
        port=host_config["port"],
    )

    def get_host_status():
        host = api_instance.get_host(host_id)["response"]
        return host["status"]

    try:
        log.info("Adding host to database")
        api_response = api_instance.add_host(host)
        host_id = api_response["response"]
        if host_id != "error":
            log.info("Installing host '%s'", host_id)
            api_instance.install_host(host_id)
            assert util.wait_for_status(
                get_host_status, status="up", timeout=120, step=5
            )

    except qarax.ApiException as e:
        log.error("Exception when calling HostsApi->add_host: %s\n" % e)
        raise e

    healthcheck = api_instance.healthcheck(host_id)
    assert healthcheck["response"] == "ok"


@pytest.mark.order(2)
def test_add_storage(api_client):

    # TODO: Something more robust will be needed in the future
    # maybe set names to the hosts and look them up by name
    host_api_instance = hosts_api.HostsApi(api_client)
    host_id = host_api_instance.list_hosts()["response"][0]["id"]

    storage = Storage(
        name="e2e-test-storage",
        storage_type="local",
        config=StorageConfig(host_id=host_id),
    )

    storage_api_instance = storage_api.StorageApi(api_client)
    try:
        log.info("Adding storage to database")
        log.info("Adding storage '%s'", storage.name)
        api_response = storage_api_instance.add_storage(storage)
        storage_id = api_response["response"]
        log.info("Added storage '%s'", storage_id)
    except qarax.ApiException as e:
        log.error("Exception when calling StorageApi->add_Storage: %s\n" % e)
        raise e

    storages = storage_api_instance.list_storage()["response"]
    assert len(storages) == 1
