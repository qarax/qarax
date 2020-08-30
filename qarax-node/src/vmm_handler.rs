extern crate firecracker_rust_sdk;

use crate::network;

use firecracker_rust_sdk::models::{
    boot_source, drive, logger, machine, machine_configuration, network_interface,
};
use node::{Status, VmConfig};

use std::convert::TryFrom;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::fs::OpenOptions;
use tokio::prelude::*;
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

    pub async fn configure_vm(&mut self, vm_config: &mut VmConfig) -> Result<()> {
        tracing::info!("Configuring VMM...");

        // TODO: do some actual validation
        let socket_path = format!("/tmp/{}.sock", vm_config.vm_id);
        let mc = machine_configuration::MachineConfiguration::new(false, vm_config.memory, 1);

        // TODO: boot_params should come from qarax and find a better way to handle kernel because it's already a string
        let mut bs = boot_source::BootSource::new(
            vm_config.kernel.to_string(),
            vm_config.kernel_params.to_string(),
        );

        let mut network_interfaces = vec![];

        // TODO: use an enum like civilized person
        if vm_config.network_mode == "dhcp" {
            network::create_tap_device(&vm_config.vm_id).await?;
            let mac: network::MacAddress;
            if vm_config.mac_address.is_empty() {
                mac = network::generate_mac();
                tracing::info!("Generated MAC address: '{}'", mac);

                // Send back the generated MAC address
                vm_config.mac_address = mac.to_string();
            } else {
                use std::str::FromStr;

                tracing::info!("Using available MAC address: '{}'", vm_config.mac_address);
                mac = network::MacAddress::from_str(&vm_config.mac_address)?;
            }

            let ip =
                network::get_ip(Arc::new(mac), Arc::new(get_tap_device(&vm_config.vm_id))).await?;

            tracing::info!("Assigning IP '{}' for VM {}", ip, &vm_config.vm_id);
            network_interfaces.push(Self::configure_network(&mut bs, &vm_config.vm_id, mac));
            vm_config.ip_address = ip;
        }

        // TODO: implement From
        let fc_drives = vm_config
            .drives
            .iter()
            .map(|drive| drive::Drive {
                drive_id: drive.drive_id.clone(),
                is_read_only: drive.is_read_only,
                is_root_device: drive.is_root_device,
                path_on_host: drive.path_on_host.clone(),
                partuuid: None,
                rate_limiter: None,
            })
            .collect();

        let mut logger = logger::Logger::new(format!("/var/log/{}.log", vm_config.vm_id));
        // TODO: get the level from qarax-node's configuration (hopefully it'll have one)
        logger.level = Some(logger::Level::Debug);
        create_log_pipe(&logger)?;

        let vmm = machine::Machine::new(
            vm_config.vm_id.to_owned(),
            socket_path,
            mc,
            bs,
            fc_drives,
            network_interfaces,
            logger,
            None,
        );

        self.machine.write().await.replace(vmm);
        Ok(())
    }

    pub async fn start_vm(&self) -> Result<()> {
        let mut machine_handler = self.machine.write().await;

        if machine_handler.is_none() {
            Err(anyhow!("No machine object!"))
        } else {
            let machine = machine_handler.as_mut().unwrap();
            let socket_path = &format!("/tmp/{}.sock", machine.vm_id);
            let config_file = self.create_config_file(machine).await?;
            let args = vec!["--api-sock", socket_path, "--config-file", &config_file];
            tracing::info!("Starting firecracker with args: '{:#?}'", args);

            let child = Command::new(FIRECRACKER_BIN)
                .args(args)
                .stdout(Stdio::null())
                .spawn()
                .expect("Faild to start firecracker");

            machine.set_pid(child.id());

            Ok(())
        }
    }

    pub async fn stop_vm(&self) -> Result<()> {
        let mut machine = self.machine.write().await;

        if machine.is_none() {
            Err(anyhow!("No machine object!"))
        } else {
            tracing::info!("Stopping VM machine...");
            let machine = machine.as_mut().unwrap();
            machine.stop().await?;
            if !machine.network_interfaces.is_empty() {
                tracing::info!("Removing tap device");
                network::delete_tap_device(&machine.vm_id).await?;
            }

            tracing::info!("VM stopped");
            Ok(())
        }
    }

    async fn create_config_file(&self, machine: &machine::Machine) -> Result<String> {
        let path = format!("/var/run/{}.config", machine.vm_id);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await?;

        let config = serde_json::to_string(&machine).unwrap();
        tracing::info!("Writing config {}", config);

        file.write_all(config.as_bytes()).await?;
        Ok(path)
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

fn create_log_pipe(logger: &logger::Logger) -> Result<()> {
    use nix::sys::stat;
    use nix::unistd;
    use std::path::Path;

    unistd::mkfifo(Path::new(&logger.log_path), stat::Mode::S_IRWXU)?;
    Ok(())
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
