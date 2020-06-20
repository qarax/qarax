use super::BootSource;
use super::MachineConfiguration;
use super::Drive;

use crate::http::client::{VmmClient, Method};

use serde_json;
use tokio;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
pub struct Machine {
    client: VmmClient,
    machine_configuration: MachineConfiguration,
    boot_source: BootSource,
    drive: Drive,
}
impl Machine {
    pub fn new(socket_path: String, machine_configuration: MachineConfiguration, boot_source: BootSource, drive: Drive) -> Self {
        Machine {
            client: VmmClient::new(socket_path),
            machine_configuration,
            boot_source,
            drive
        }
    }

    // TODO: check errors and stuff
    pub async fn configure_boot_source(&self) -> Result<String> {
        let boot_source = serde_json::to_string(&self.boot_source).unwrap();
        println!("Sending boot_source with {}\n", boot_source);
        Ok(self.client.request("/boot-source", Method::PUT, &boot_source.as_bytes()).await?)
    }

    pub async fn configure_drive(&self) -> Result<String> {
        let drive = serde_json::to_string(&self.drive).unwrap();
        println!("Sending drive with {}\n", drive);
        let drive_id = &self.drive.drive_id;
        let endpoint = format!("/drives/{}", drive_id);
        Ok(self.client.request(&endpoint, Method::PUT, &drive.as_bytes()).await?)
    }

    pub async fn start(&self) -> Result<String> {   
        tokio::join!(self.configure_boot_source(), self.configure_drive());
     
        println!("Starting VM :O");
        Ok(self.client.request("/actions", Method::PUT,b"{\"action_type\": \"InstanceStart\"}").await?)
    }
}