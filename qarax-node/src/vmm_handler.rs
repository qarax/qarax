extern crate firecracker_rust_sdk;

use crate::network;
use firecracker_rust_sdk::models::{
    boot_source, drive, logger, machine, machine_configuration, network_interface,
};
use node::{Status, VmConfig};

use std::convert::TryFrom;
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::RwLock;

use std::sync::Arc;

pub(crate) mod node {
    tonic::include_proto!("node");
}

type AsyncResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

    pub async fn configure_vm(&mut self, vm_config: &VmConfig) {
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
            network = Some(Self::configure_network(&mut bs, &vm_config.vm_id));
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
            network::create_tap_device(&vm_config.vm_id).await;
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

            // TODO: find a better way to do polling and definitly do not block the request
            use std::{thread, time};
            thread::sleep(time::Duration::from_millis(5000));

            // TODO: there has got to be a better way
            let mac = m
                .as_ref()
                .unwrap()
                .network
                .as_ref()
                .unwrap()
                .guest_mac
                .as_ref()
                .unwrap();
            // TODO: use -q, -x, -I options
            let arp_scan = Command::new("arp-scan").arg("-l").output().await;
            let ip =
                network::get_ip(String::from_utf8(arp_scan.unwrap().stdout).unwrap(), &mac).await;
            tracing::info!("ip address: {}", ip);
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
    ) -> network_interface::NetworkInterface {
        // TODO: implement static ip as well
        bs.boot_args = format!("{} ip=dhcp", bs.boot_args);

        network_interface::NetworkInterface {
            guest_mac: Some(network::generate_mac()),
            host_dev_name: format!("fc-tap-{}", &vm_id[..4]),
            iface_id: String::from("1"), // TODO: assign a normal ID
            allow_mmds_requests: None,
            rx_rate_limiter: None,
            tx_rate_limiter: None,
        }
    }
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
