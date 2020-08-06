use super::BootSource;
use super::Drive;
use super::Logger;
use super::MachineConfiguration;
use super::NetworkInterface;

use crate::http::client::{Method, VmmClient};

use anyhow::Result;

#[derive(Debug)]
pub struct Machine {
    pub vm_id: String,
    client: VmmClient,
    machine_configuration: MachineConfiguration,
    boot_source: BootSource,
    drive: Drive,
    pub network: Option<NetworkInterface>,
    logger: Logger,
    pid: u32,
}

impl Machine {
    pub fn new(
        vm_id: String,
        socket_path: String,
        machine_configuration: MachineConfiguration,
        boot_source: BootSource,
        drive: Drive,
        network: Option<NetworkInterface>,
        logger: Logger,
        pid: u32,
    ) -> Self {
        Machine {
            vm_id,
            client: VmmClient::new(socket_path),
            machine_configuration,
            boot_source,
            drive,
            network,
            logger,
            pid,
        }
    }

    // TODO: check errors and stuff
    pub async fn configure_boot_source(&self) -> Result<String> {
        let boot_source = serde_json::to_string(&self.boot_source)?;
        tracing::info!("Sending boot_source with {}\n", boot_source);

        Ok(self
            .client
            .request("/boot-source", Method::PUT, &boot_source.as_bytes())
            .await?)
    }

    pub async fn configure_drive(&self) -> Result<String> {
        let drive = serde_json::to_string(&self.drive)?;

        tracing::info!("Sending drive with {}\n", drive);

        let drive_id = &self.drive.drive_id;
        let endpoint = format!("/drives/{}", drive_id);

        Ok(self
            .client
            .request(&endpoint, Method::PUT, &drive.as_bytes())
            .await?)
    }

    pub async fn configure_logger(&self) -> Result<String> {
        // TODO: error handling
        use nix::sys::stat;
        use nix::unistd;
        use std::path::Path;

        unistd::mkfifo(Path::new(&self.logger.log_path), stat::Mode::S_IRWXU)?;

        let logger = serde_json::to_string(&self.logger)?;

        tracing::info!("Sending logger with {}\n", logger);

        Ok(self
            .client
            .request("/logger", Method::PUT, &logger.as_bytes())
            .await?)
    }

    pub async fn configure_network(&self) -> Result<String> {
        let network_definition = self.network.as_ref().unwrap();
        let network = serde_json::to_string(network_definition)?;
        tracing::info!("Sending network with {}\n", network);
        let endpoint = format!("/network-interfaces/{}", network_definition.iface_id);

        Ok(self
            .client
            .request(&endpoint, Method::PUT, &network.as_bytes())
            .await?)
    }

    pub async fn start(&self) -> Result<String> {
        tracing::info!("Starting VM :O");

        Ok(self
            .client
            .request(
                "/actions",
                Method::PUT,
                b"{\"action_type\": \"InstanceStart\"}",
            )
            .await?)
    }

    pub async fn stop(&self) -> Result<()> {
        // TODO error handling
        use nix::sys::signal;
        use nix::sys::wait::waitpid;
        use nix::unistd::Pid;
        use std::fs;

        signal::kill(Pid::from_raw(self.pid as i32), signal::Signal::SIGTERM)?;
        waitpid(Pid::from_raw(self.pid as i32), None)?;
        fs::remove_file(&self.client.socket_path).expect("failed to remove file");

        Ok(())
    }
}
