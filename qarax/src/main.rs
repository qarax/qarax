use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use qarax::{
    configuration::{default_control_plane_architecture, get_configuration},
    database,
    grpc_client::NodeTls,
    startup::run,
};

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

    let node_tls = NodeTls::load(&configuration.node_tls_dir).map_err(|e| {
        tracing::error!("Failed to load the node TLS files: {e:#}");
        std::io::Error::other(e)
    })?;

    database::run_migrations(&configuration.database.connection_string())
        .await
        .expect("Failed to run migrations");

    let address = format!(
        "{}:{}",
        configuration.application.host, configuration.application.port
    );

    let connection_pool = database::connect_lazy(&configuration.database);
    tracing::info!("Starting server on {}", address);
    let listener = TcpListener::bind(address).await?;
    let vm_defaults = configuration.vm_defaults.clone();
    let scheduling = configuration.scheduling.clone();
    let sandbox = configuration.sandbox.clone();
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
        sandbox,
        configuration.auth.clone(),
        configuration.ha.clone(),
        default_control_plane_architecture(),
        node_tls,
        configuration.ui_dir.clone(),
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
            return Err(std::io::Error::other(e));
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
