use super::BootSource;
use super::MachineConfiguration;
use super::Drive;

use crate::http::client::{VmmClient, Method};

use serde_json;
use tokio;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Machine<'a> {
    pub socket_path: &'a str,
    pub machine_configuration: MachineConfiguration,
    pub boot_source: BootSource,
    pub drive: Drive,
}
impl<'a> Machine<'a> {
    
    // TODO: return errors and stuff
    pub async fn start(&self) {
        let client = VmmClient::new(self.socket_path);

        let boot_source = serde_json::to_string(&self.boot_source).unwrap();
        println!("Sending PUT with {}", boot_source);
        let boot_source_request = client.request("/boot-source", Method::PUT, &boot_source.as_bytes());
        
        let drive = serde_json::to_string(&self.drive).unwrap();
        println!("Sending PUT with {}", drive);
        let drive_id = &self.drive.drive_id;
        let endpoint = format!("/drives/{}", drive_id);
        let drive_request = client.request(&endpoint, Method::PUT, &drive.as_bytes());
        tokio::join!(boot_source_request, drive_request);

        println!("Starting instance :O");
        client.request("/actions", Method::PUT,b"{\"action_type\": \"InstanceStart\"}").await.unwrap();
    }
}