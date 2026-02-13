use clap::Parser;
use tonic::transport::Server;
use tracing::{Level, info};
use tracing_subscriber;

use qarax_node::rpc::node::vm_service_server::VmServiceServer;
use qarax_node::services::vm::VmServiceImpl;

#[derive(Parser, Debug)]
#[clap(
    name = "qarax-node",
    rename_all = "kebab-case",
    rename_all_env = "screaming-snake"
)]
pub struct Args {
    #[clap(short, long, default_value = "50051")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let args = Args::parse();
    let addr = format!("0.0.0.0:{}", args.port).parse()?;

    info!("qarax-node starting on {}", addr);

    // Create the VM service
    let vm_service = VmServiceImpl::new();

    info!("Starting gRPC server (NOOP mode - VMs will not actually be created)");

    // Start the gRPC server
    Server::builder()
        .add_service(VmServiceServer::new(vm_service))
        .serve(addr)
        .await?;

    Ok(())
}
