use super::BootSource;
use super::MachineConfiguration;

use crate::http::client::{VmmClient, Method};


#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Machine<'a> {
    pub socket_path: &'a str,
    pub machine_configuration: MachineConfiguration,
    pub boot_source: BootSource,

}
impl<'a> Machine<'a> {
    pub async fn start(&self) {
        let client = VmmClient::new(self.socket_path);

        println!("{}", client.request("/", Method::GET, b"").await.unwrap());
    }
}