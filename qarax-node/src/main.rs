mod vm_service;
mod vmm_handler;

use tonic::transport::Server;
use vm_service::VmService;
use vmm_handler::node::node_server::NodeServer;

use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "0.0.0.0:50051".parse()?;
    let vm_service = VmService::new();

    Server::builder()
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .add_service(NodeServer::new(vm_service))
        .serve(addr)
        .await?;

    Ok(())
}
