use clap::Clap;
use dotenv::dotenv;
use std::{error::Error, net::SocketAddr};

mod database;
mod env;
mod handlers;

#[derive(Clap, Debug)]
#[clap(
    name = "qarax",
    rename_all = "kebab-case",
    rename_all_env = "screaming-snake"
)]
pub struct Args {
    #[clap(short, long, default_value = "3000", env)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "qarax=debug,tower_http=debug")
    }

    tracing_subscriber::fmt::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let db_url = &dotenv::var("DATABASE_URL").expect("DATABASE_URL is not set!");
    database::run_migrations(db_url).await?;
    let pool = database::connect(db_url).await?;
    let environment = env::Environment::new(pool).await?;

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(handlers::app(environment).into_make_service())
        .await?;

    Ok(())
}
