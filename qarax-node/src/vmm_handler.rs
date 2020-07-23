extern crate firecracker_rust_sdk;

use crate::network;
use crate::network::IpAddr;

use firecracker_rust_sdk::models::{
    boot_source, drive, logger, machine, machine_configuration, network_interface,
};
use node::{Status, VmConfig};

use std::convert::TryFrom;
use std::process::Stdio;
use std::sync::Arc;

use tokio::process::Command;
use tokio::sync::RwLock;

pub(crate) mod node {
    tonic::include_proto!("node");
}

// Make configurable
const FIRECRACKER_BIN: &str = "./firecracker";

// TODO: VmmHandler sounds like a stupid name
#[derive(Debug)]
pub struct VmmHandler {
    machine: Arc<RwLock<Option<machine::Machine>>>,
}

impl VmmHandler {
    pub fn new() -> Self {
        VmmHandler {
            machine: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn configure_vm(&mut self, vm_config: &mut VmConfig) {
        tracing::info!("Configuring VMM...");

        // TODO: do some actual validation
        let socket_path = format!("/tmp/{}.sock", vm_config.vm_id);
        let mc = machine_configuration::MachineConfiguration::new(false, vm_config.memory, 1);

        // TODO: boot_params should come from qarax and find a better way to handle kernel because it's already a string
        let mut bs = boot_source::BootSource::new(
            vm_config.kernel.to_string(),
            vm_config.kernel_params.to_string(),
        );

        let mut network = None;

        // TODO: use an enum like civilized person
        if vm_config.network_mode == "dhcp" {
            network::create_tap_device(&vm_config.vm_id).await;
            let mac = network::generate_mac();
            tracing::info!("Generated MAC address: '{}'", mac);

            // TODO: The IP should be sent back to qarax
            let ip = network::get_ip(Arc::new(mac), Arc::new(get_tap_device(&vm_config.vm_id)))
                .await
                .unwrap();

            tracing::info!("Assigning IP '{}' for VM {}", ip, &vm_config.vm_id);
            network = Some(Self::configure_network(&mut bs, &vm_config.vm_id, mac));
            vm_config.address = ip;
        }

        // TODO: move into own function and handle errors
        // - check if socket exists before
        // - make a sanity check on the api server
        tracing::info!("Starting FC process...");

        let child = Command::new(FIRECRACKER_BIN)
            .args(vec!["--api-sock", &socket_path])
            .stdout(Stdio::null())
            .spawn()
            .expect("Faild to start firecracker");

        // TODO: proper polling that the api server is
        // available required here
        use std::{thread, time};
        thread::sleep(time::Duration::from_millis(1000));

        // TODO: get paths and ids from qarax
        let drive = drive::Drive::new(String::from("rootfs"), false, true, String::from("rootfs"));
        let mut logger = logger::Logger::new(format!("/var/log/{}.log", vm_config.vm_id));
        // TODO: get the level from qarax-node's configuration (hopefully it'll have one)
        logger.level = Some(logger::Level::Debug);

        let vmm = machine::Machine::new(
            vm_config.vm_id.to_owned(),
            socket_path,
            mc,
            bs,
            drive,
            network,
            logger,
            child.id(),
        );
        vmm.configure_logger().await;

        if vmm.network.is_some() {
            tracing::info!("Configuring network...");
            vmm.configure_network().await;
        }

        tracing::info!("Waiting for configuration...");
        tokio::join!(vmm.configure_boot_source(), vmm.configure_drive(),);

        self.machine.write().await.replace(vmm);
    }

    pub async fn start_vm(&self) {
        let m = self.machine.read().await;
        if m.is_some() {
            tracing::info!("Starting VM machine...");
            m.as_ref().unwrap().start().await;
            tracing::info!("machine started");
        } else {
            tracing::error!("Machine object unavilable! - start");
        }
    }

    pub async fn stop_vm(&self) {
        let m = self.machine.read().await;
        if m.is_some() {
            tracing::info!("Stopping VM machine...");
            let machine = m.as_ref().unwrap();
            match machine.stop().await {
                Ok(_) => {
                    if machine.network.is_some() {
                        tracing::info!("Removing tap device");
                        network::delete_tap_device(&machine.vm_id).await;
                    }
                    tracing::info!("VM stopped")
                }
                Err(e) => tracing::error!("Failed to stop VM :( {}", e.to_string()),
            }
        } else {
            tracing::error!("Machine object unavilable! - stop");
        }
    }

    fn configure_network(
        bs: &mut boot_source::BootSource,
        vm_id: &str,
        mac: network::MacAddress,
    ) -> network_interface::NetworkInterface {
        // TODO: implement static ip as well
        bs.boot_args = format!("{} ip=dhcp", bs.boot_args);

        network_interface::NetworkInterface {
            guest_mac: Some(mac.to_string()),
            host_dev_name: get_tap_device(vm_id),
            iface_id: String::from("1"), // TODO: assign a normal ID
            allow_mmds_requests: None,
            rx_rate_limiter: None,
            tx_rate_limiter: None,
        }
    }
}

// TODO: turn this into a macro or something
fn get_tap_device(vm_id: &str) -> String {
    format!("fc-tap-{}", &vm_id[..4])
}

impl TryFrom<i32> for Status {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Status::Success),
            1 => Ok(Status::Failure),
            _ => panic!("Shouldn't happen"),
        }
    }
}
