use std::net::SocketAddr;

use clap::Parser;
use common::telemtry::{get_subscriber, init_subscriber};
use qarax_node::startup::run;

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
    let args = Args::parse();

    let subscriber = get_subscriber("qarax-node".into(), "info".into(), std::io::stdout);
    init_subscriber(subscriber);
    match run().await {
        Ok(r) => {
            tracing::info!("qarax-node is running on port {}", args.port);
            let addr: SocketAddr = SocketAddr::from(([0, 0, 0, 0], args.port));
            r.serve(addr).await?;
        }
        Err(e) => {
            tracing::error!("qarax-node failed to start: {}", e);
        }
    }

    Ok(())
}
