use std::net::SocketAddr;

use clap::Parser;
use common::telemtry::{get_subscriber, init_subscriber};

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

    Ok(())
}
