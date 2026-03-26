use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tonic::transport::Server;
#[cfg(not(feature = "otel"))]
use tracing::Level;
use tracing::info;

use qarax_node::cloud_hypervisor::VmManager;
use qarax_node::image_store::ImageStoreManager;
use qarax_node::overlaybd::OverlayBdManager;
use qarax_node::rpc::node::file_transfer_service_server::FileTransferServiceServer;
use qarax_node::rpc::node::vm_service_server::VmServiceServer;
use qarax_node::services::file_transfer::FileTransferServiceImpl;
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

    /// Path to virtiofsd binary
    #[clap(long, default_value = "/usr/local/bin/virtiofsd")]
    virtiofsd_binary: PathBuf,

    /// Path to qarax-init binary (injected into OCI VMs as the init process)
    #[clap(long, default_value = "/usr/local/bin/qarax-init")]
    qarax_init_binary: PathBuf,

    /// Directory for OCI image cache
    #[clap(long, default_value = "/var/lib/qarax/images")]
    image_cache_dir: PathBuf,

    /// Path to convertor binary (OverlayBD image converter from accelerated-container-image)
    #[clap(long, default_value = "/opt/overlaybd/snapshotter/convertor")]
    convertor_binary: PathBuf,

    /// Directory for OverlayBD cache and per-VM config files
    #[clap(long, default_value = "/var/lib/qarax/overlaybd")]
    overlaybd_cache_dir: PathBuf,

    /// Enable OpenTelemetry export
    #[clap(long, default_value = "false", env = "OTEL_ENABLED")]
    otel_enabled: bool,

    /// OTLP HTTP endpoint
    #[clap(
        long,
        default_value = "http://localhost:4318",
        env = "OTEL_EXPORTER_OTLP_ENDPOINT"
    )]
    otel_endpoint: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Optionally initialize OpenTelemetry before tracing subscriber
    #[cfg(feature = "otel")]
    let _otel_guard = if args.otel_enabled {
        let otel_config = common::otel::OtelConfig {
            service_name: "qarax-node".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            otlp_endpoint: args.otel_endpoint.clone(),
        };
        match common::otel::init_providers(otel_config) {
            Ok(guard) => {
                eprintln!("OpenTelemetry enabled, exporting to {}", args.otel_endpoint);
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

    // Initialize tracing (with OTel layer when enabled)
    #[cfg(feature = "otel")]
    {
        let subscriber =
            common::telemtry::get_subscriber("qarax-node".into(), "info".into(), std::io::stdout);
        common::telemtry::init_subscriber(subscriber);
    }
    #[cfg(not(feature = "otel"))]
    {
        tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    }

    let addr = format!("0.0.0.0:{}", args.port).parse()?;

    info!("qarax-node starting on {}", addr);
    info!("Runtime directory: {}", args.runtime_dir.display());
    info!(
        "Cloud Hypervisor binary: {}",
        args.cloud_hypervisor_binary.display()
    );
    info!("virtiofsd binary: {}", args.virtiofsd_binary.display());
    info!("qarax-init binary: {}", args.qarax_init_binary.display());
    info!("Image cache dir: {}", args.image_cache_dir.display());
    info!("convertor binary: {}", args.convertor_binary.display());
    info!(
        "OverlayBD cache dir: {}",
        args.overlaybd_cache_dir.display()
    );

    // Ensure directories exist
    tokio::fs::create_dir_all(&args.runtime_dir).await?;
    tokio::fs::create_dir_all(&args.image_cache_dir).await?;
    tokio::fs::create_dir_all(&args.overlaybd_cache_dir).await?;

    // Build ImageStoreManager if virtiofsd is present
    let image_store_manager = if args.virtiofsd_binary.exists() {
        info!("virtiofsd found — OCI image boot enabled");
        Some(Arc::new(ImageStoreManager::new(
            &args.virtiofsd_binary,
            &args.qarax_init_binary,
            &args.image_cache_dir,
            &args.runtime_dir,
        )))
    } else {
        info!(
            "virtiofsd not found at {} — OCI image boot disabled",
            args.virtiofsd_binary.display()
        );
        None
    };

    // Build OverlayBdManager if convertor binary is present
    let overlaybd_manager = if args.convertor_binary.exists() {
        info!("convertor found — OverlayBD lazy image boot enabled");
        let mgr = Arc::new(OverlayBdManager::new(
            &args.convertor_binary,
            &args.overlaybd_cache_dir,
        ));
        mgr.recover().await;
        Some(mgr)
    } else {
        info!(
            "convertor not found at {} — OverlayBD disabled",
            args.convertor_binary.display()
        );
        None
    };

    // Build VmManager with optional ImageStoreManager and OverlayBdManager
    let qarax_init_binary = if args.qarax_init_binary.exists() {
        Some(args.qarax_init_binary.clone())
    } else {
        info!(
            "qarax-init not found at {} — init injection disabled",
            args.qarax_init_binary.display()
        );
        None
    };

    let vm_manager = Arc::new(VmManager::with_overlaybd(
        &args.runtime_dir,
        &args.cloud_hypervisor_binary,
        image_store_manager,
        overlaybd_manager,
        qarax_init_binary,
    ));
    vm_manager.recover_vms().await;

    // Create the VM service from the manager
    let vm_service = VmServiceImpl::from_manager(vm_manager);

    info!("Starting gRPC server with Cloud Hypervisor backend");

    let file_transfer_service = FileTransferServiceImpl::new();

    // Start the gRPC server (with trace extraction layer when otel is enabled)
    #[cfg(feature = "otel")]
    {
        Server::builder()
            .layer(trace_extraction::OtelGrpcLayer)
            .add_service(VmServiceServer::new(vm_service))
            .add_service(FileTransferServiceServer::new(file_transfer_service))
            .serve(addr)
            .await?;
    }

    #[cfg(not(feature = "otel"))]
    {
        Server::builder()
            .add_service(VmServiceServer::new(vm_service))
            .add_service(FileTransferServiceServer::new(file_transfer_service))
            .serve(addr)
            .await?;
    }

    Ok(())
}

/// Tower layer that extracts W3C trace context from incoming gRPC metadata
/// and sets it as the parent context on the current tracing span.
#[cfg(feature = "otel")]
mod trace_extraction {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use http::Request;
    use tower::Layer;
    use tower::Service;

    /// Extracts OpenTelemetry context from HTTP headers.
    pub struct HeaderExtractor<'a>(pub &'a http::HeaderMap);

    impl opentelemetry::propagation::Extractor for HeaderExtractor<'_> {
        fn get(&self, key: &str) -> Option<&str> {
            self.0.get(key).and_then(|v| v.to_str().ok())
        }

        fn keys(&self) -> Vec<&str> {
            self.0.keys().map(|k| k.as_str()).collect()
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub struct OtelGrpcLayer;

    impl<S: Clone> Layer<S> for OtelGrpcLayer {
        type Service = OtelGrpcService<S>;

        fn layer(&self, inner: S) -> Self::Service {
            OtelGrpcService { inner }
        }
    }

    #[derive(Debug, Clone)]
    pub struct OtelGrpcService<S> {
        inner: S,
    }

    impl<S, B> Service<Request<B>> for OtelGrpcService<S>
    where
        S: Service<Request<B>> + Clone + Send + 'static,
        S::Future: Send + 'static,
        B: Send + 'static,
    {
        type Response = S::Response;
        type Error = S::Error;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, request: Request<B>) -> Self::Future {
            use tracing::Instrument;
            use tracing_opentelemetry::OpenTelemetrySpanExt;

            // Extract trace context from HTTP/gRPC headers
            let parent_context = opentelemetry::global::get_text_map_propagator(|propagator| {
                propagator.extract(&HeaderExtractor(request.headers()))
            });

            let span = tracing::info_span!("grpc.request");
            let _ = span.set_parent(parent_context);

            Box::pin(self.inner.call(request).instrument(span))
        }
    }
}
