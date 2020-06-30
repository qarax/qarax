mod vm_service;
mod vmm_handler;

use tonic::transport::Server;
use vm_service::VmService;
use vmm_handler::node::node_server::NodeServer;

use std::env;
use std::time::Duration;
use tracing;
use tracing_appender;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_path = "qarax-node.log";

    let file_appender = tracing_appender::rolling::hourly(env::current_dir().unwrap(), log_path);
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt().with_writer(non_blocking).init();

    let addr = "0.0.0.0:50051".parse()?;

    let vm_service = VmService::new();

    tracing::info!("Starting server on port 50051...");

    Server::builder()
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .add_service(NodeServer::new(vm_service))
        .serve(addr)
        .await?;

    Ok(())
}
