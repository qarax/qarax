mod vmm_handler;
mod vm_service;

use vmm_handler::node::node_server::NodeServer;
use vm_service::VmService;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let vm_service = VmService::new();

    Server::builder()
        .add_service(NodeServer::new(vm_service))
        .serve(addr)
        .await?;

    Ok(())
}
