#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_term;

mod vm_service;
mod vmm_handler;

use tonic::transport::Server;
use vm_service::VmService;
use vmm_handler::node::node_server::NodeServer;

use std::time::Duration;

use slog::Drain;
use std::fs::OpenOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_path = "qarax-node.log";
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .unwrap();

    let decorator = slog_term::PlainDecorator::new(file);
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    let _log = slog::Logger::root(drain, o!());
    let addr = "0.0.0.0:50051".parse()?;
    let vm_service = VmService::new(slog::Logger::root(_log.clone(), o!()));

    info!(_log, "Starting server on port 50051...");
    Server::builder()
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .add_service(NodeServer::new(vm_service))
        .serve(addr)
        .await?;

    Ok(())
}
