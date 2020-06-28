mod vm_service;
mod vmm_handler;

use tonic::transport::Server;
use vm_service::VmService;
use vmm_handler::node::node_server::NodeServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "0.0.0.0:50051".parse()?;
    let vm_service = VmService::new();

    Server::builder()
        .add_service(NodeServer::new(vm_service))
        .serve(addr)
        .await?;

    Ok(())
}
