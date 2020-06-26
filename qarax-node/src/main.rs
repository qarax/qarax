mod vmm_handler;
mod vm_service;

use vmm_handler::node::node_server::NodeServer;
use vm_service::VmService;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "0.0.0.0:50051".parse()?;
    let node = QaraxNode::default();

    Server::builder()
        .add_service(NodeServer::new(vm_service))
        .serve(addr)
        .await?;

    Ok(())
}
