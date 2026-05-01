use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use qarax::{
    configuration::{default_control_plane_architecture, get_configuration},
    database,
    startup::run,
};
use sqlx::PgPool;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    let configuration = get_configuration().expect("Failed to read configuration.");

    // Optionally initialize OpenTelemetry before the tracing subscriber
    #[cfg(feature = "otel")]
    let _otel_guard = if configuration.telemetry.otel_enabled {
        let otel_config = common::otel::OtelConfig {
            service_name: "qarax".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            otlp_endpoint: configuration.telemetry.otlp_endpoint.clone(),
        };
        match common::otel::init_providers(otel_config) {
            Ok(guard) => {
                eprintln!(
                    "OpenTelemetry enabled, exporting to {}",
                    configuration.telemetry.otlp_endpoint
                );
                Some(guard)
            }
            Err(e) => {
                eprintln!("Failed to initialize OpenTelemetry: {e}");
                None
            }
        }
    } else {
        None
    };

    let subscriber = get_subscriber("qarax".into(), "info".into(), std::io::stdout);
    init_subscriber(subscriber);

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
    let vm_defaults = configuration.vm_defaults.clone();
    let scheduling = configuration.scheduling.clone();
    tracing::info!(
        "VM defaults: kernel={}, initramfs={:?}, cmdline={}",
        vm_defaults.kernel,
        vm_defaults.initramfs,
        vm_defaults.cmdline
    );
    match run(
        listener,
        connection_pool,
        configuration.database.clone(),
        vm_defaults,
        scheduling,
        default_control_plane_architecture(),
    )
    .await
    {
        Ok(server) => {
            tokio::select! {
                result = async { server.await } => result.unwrap(),
                _ = shutdown_signal() => {}
            }
        }
        Err(e) => {
            tracing::error!("Server failed to start: {}", e);
        }
    }

    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = sigterm.recv() => {},
    }
}
