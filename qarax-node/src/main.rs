mod lib;
mod server;

use lib::node::node_server::NodeServer;
use server::QaraxNode;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let node = QaraxNode::default();

    Server::builder()
        .add_service(NodeServer::new(node))
        .serve(addr)
        .await?;

    Ok(())
}
