use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use qarax::{configuration::get_configuration, database, startup::run};
use sqlx::PgPool;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let subscriber = get_subscriber("qarax".into(), "info".into(), std::io::stdout);
    init_subscriber(subscriber);

    let configuration = get_configuration().expect("Failed to read configuration.");
    database::run_migrations(&configuration.database.connection_string())
        .await
        .expect("Failed to run migrations");

    let address = format!(
        "{}:{}",
        configuration.application.host, configuration.application.port
    );

    let db_options = configuration.database.without_db();
    let connection_pool = PgPool::connect_lazy_with(db_options);
    tracing::info!("Starting server on {}", address);
    let listener = TcpListener::bind(address).await?;
    match run(listener, connection_pool).await {
        Ok(server) => {
            server.await.unwrap();
        }
        Err(e) => {
            tracing::error!("Server failed to start: {}", e);
        }
    }

    Ok(())
}
