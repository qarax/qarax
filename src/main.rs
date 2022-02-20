use clap::Parser;
use dotenv::dotenv;
use std::{error::Error, net::SocketAddr};

use common::telemetry::{get_subscriber, init_subscriber};

mod database;
mod env;
mod handlers;

#[derive(Parser, Debug)]
#[clap(
    name = "qarax",
    rename_all = "kebab-case",
    rename_all_env = "screaming-snake"
)]
pub struct Args {
    #[clap(short, long, default_value = "3000")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    dotenv().ok();

    let subscriber = get_subscriber("qarax".into(), "info".into(), std::io::stdout);
    init_subscriber(subscriber);

    let db_url = &dotenv::var("DATABASE_URL").expect("DATABASE_URL is not set!");
    database::run_migrations(db_url).await?;
    let pool = database::connect(db_url).await?;
    let environment = env::Environment::new(pool).await?;

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(handlers::app(environment).await.into_make_service())
        .await?;

    Ok(())
}
