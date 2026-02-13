use clap::Parser;
use std::path::PathBuf;
use tonic::transport::Server;
use tracing::{Level, info};

use qarax_node::rpc::node::vm_service_server::VmServiceServer;
use qarax_node::services::vm::VmServiceImpl;

#[derive(Parser, Debug)]
#[clap(
    name = "qarax-node",
    about = "qarax data plane - manages VMs using Cloud Hypervisor",
    rename_all = "kebab-case",
    rename_all_env = "screaming-snake"
)]
pub struct Args {
    /// Port to listen on
    #[clap(short, long, default_value = "50051")]
    port: u16,

    /// Runtime directory for VM sockets and logs
    #[clap(long, default_value = "/var/lib/qarax/vms")]
    runtime_dir: PathBuf,

    /// Path to cloud-hypervisor binary
    #[clap(long, default_value = "/usr/local/bin/cloud-hypervisor")]
    cloud_hypervisor_binary: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let args = Args::parse();
    let addr = format!("0.0.0.0:{}", args.port).parse()?;

    info!("qarax-node starting on {}", addr);
    info!("Runtime directory: {}", args.runtime_dir.display());
    info!(
        "Cloud Hypervisor binary: {}",
        args.cloud_hypervisor_binary.display()
    );

    // Ensure runtime directory exists
    tokio::fs::create_dir_all(&args.runtime_dir).await?;

    // Create the VM service
    let vm_service = VmServiceImpl::with_paths(&args.runtime_dir, &args.cloud_hypervisor_binary);

    info!("Starting gRPC server with Cloud Hypervisor backend");

    // Start the gRPC server
    Server::builder()
        .add_service(VmServiceServer::new(vm_service))
        .serve(addr)
        .await?;

    Ok(())
}
