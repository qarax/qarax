mod vm;

use vm::vmm_service::VmmService;

use clap::Clap;
use std::net::SocketAddr;
use std::time::Duration;
use tonic::transport::Server;
use vm::node::node_server::NodeServer;

#[derive(Clap, Debug)]
#[clap(
    name = "qarax-node",
    rename_all = "kebab-case",
    rename_all_env = "screaming-snake"
)]
pub struct Args {
    #[clap(short, long, default_value = "50051", env)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "qarax-node=debug")
    }

    tracing_subscriber::fmt::fmt().init();

    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<NodeServer<VmmService>>()
        .await;

    tracing::info!("Starting on port {}", args.port);
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));

    Server::builder()
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .add_service(health_service)
        .add_service(NodeServer::new(VmmService::default()))
        .serve(addr)
        .await?;

    Ok(())
}
