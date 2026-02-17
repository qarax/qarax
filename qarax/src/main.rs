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
    let qarax_node_address = configuration.qarax_node.address();
    let vm_defaults = configuration.vm_defaults.clone();
    tracing::info!("qarax-node address: {}", qarax_node_address);
    tracing::info!(
        "VM defaults: kernel={}, initramfs={:?}, cmdline={}",
        vm_defaults.kernel,
        vm_defaults.initramfs,
        vm_defaults.cmdline
    );
    match run(listener, connection_pool, qarax_node_address, vm_defaults).await {
        Ok(server) => {
            server.await.unwrap();
        }
        Err(e) => {
            tracing::error!("Server failed to start: {}", e);
        }
    }

    Ok(())
}
