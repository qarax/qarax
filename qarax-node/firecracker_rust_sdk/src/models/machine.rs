use super::BootSource;
use super::Drive;
use super::Logger;
use super::MachineConfiguration;
use super::NetworkInterface;

use crate::http::client::{Method, VmmClient};

use anyhow::Result;

#[derive(Serialize, Debug)]
pub struct Machine {
    #[serde(skip_serializing)]
    pub vm_id: String,

    #[serde(skip_serializing)]
    client: VmmClient,

    #[serde(rename(serialize = "machine-config"))]
    machine_configuration: MachineConfiguration,

    #[serde(rename(serialize = "boot-source"))]
    boot_source: BootSource,
    drives: Vec<Drive>,

    #[serde(rename(serialize = "network-interfaces"))]
    pub network_interfaces: Vec<NetworkInterface>,
    logger: Logger,

    #[serde(skip_serializing)]
    pid: Option<u32>,
}

impl Machine {
    pub fn new(
        vm_id: String,
        socket_path: String,
        machine_configuration: MachineConfiguration,
        boot_source: BootSource,
        drives: Vec<Drive>,
        network_interfaces: Vec<NetworkInterface>,
        logger: Logger,
        pid: Option<u32>,
    ) -> Self {
        Machine {
            vm_id,
            client: VmmClient::new(socket_path),
            machine_configuration,
            boot_source,
            drives,
            network_interfaces,
            logger,
            pid,
        }
    }

    pub async fn configure_boot_source(&self) -> Result<String> {
        let boot_source = serde_json::to_string(&self.boot_source)?;
        tracing::info!("Sending boot_source with {}\n", boot_source);

        Ok(self
            .client
            .request("/boot-source", Method::PUT, &boot_source.as_bytes())
            .await?)
    }

    pub async fn configure_drive(&self) -> Result<String> {
        let mut response = String::new();
        for drive in &self.drives {
            let drive_json = serde_json::to_string(&drive)?;

            tracing::info!("Sending drive with {}\n", drive_json);

            let drive_id = &drive.drive_id;
            let endpoint = format!("/drives/{}", drive_id);

            response = self
                .client
                .request(&endpoint, Method::PUT, &drive_json.as_bytes())
                .await?;
        }

        Ok(response)
    }

    pub async fn configure_logger(&self) -> Result<String> {
        let logger = serde_json::to_string(&self.logger)?;

        tracing::info!("Sending logger with {}\n", logger);

        Ok(self
            .client
            .request("/logger", Method::PUT, &logger.as_bytes())
            .await?)
    }

    pub async fn configure_network(&self) -> Result<String> {
        let mut response = String::new();
        for network in &self.network_interfaces {
            let network_definition = serde_json::to_string(&network)?;
            tracing::info!("Sending network with {}\n", network_definition);
            let endpoint = format!("/network-interfaces/{}", network.iface_id);

            response = self
                .client
                .request(&endpoint, Method::PUT, &network_definition.as_bytes())
                .await?;
        }

        Ok(response)
    }

    pub async fn start(&self) -> Result<String> {
        Ok(self
            .client
            .request(
                "/actions",
                Method::PUT,
                b"{\"action_type\": \"InstanceStart\"}",
            )
            .await?)
    }

    pub async fn stop(&mut self) -> Result<()> {
        use nix::sys::signal;
        use nix::sys::wait::waitpid;
        use nix::unistd::Pid;
        use std::fs;

        let pid = &self.pid.take().unwrap();
        signal::kill(Pid::from_raw(*pid as i32), signal::Signal::SIGTERM)?;
        waitpid(Pid::from_raw(*pid as i32), None)?;
        fs::remove_file(&self.client.socket_path).expect("failed to socket file");
        fs::remove_file(&self.logger.log_path).expect("failed to log file");

        Ok(())
    }

    pub fn set_pid(&mut self, pid: u32) {
        self.pid.replace(pid);
    }
}
