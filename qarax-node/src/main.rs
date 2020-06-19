mod server;

use server::node::node_server::NodeServer;
use server::QaraxNode;
use tonic::transport::Server;
use qarax_node::create_firecracker_client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let node = QaraxNode::default();

    create_firecracker_client().await;

    Server::builder()
        .add_service(NodeServer::new(node))
        .serve(addr)
        .await?;

    Ok(())
}
