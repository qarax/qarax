
use tonic::{transport::Server};
use server::QaraxNode;
use server::node::node_server::NodeServer;

mod server;


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