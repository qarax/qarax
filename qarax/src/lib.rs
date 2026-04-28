pub mod configuration;
pub mod database;
pub mod errors;
pub mod grpc_client;
pub mod handlers;
pub mod hook_executor;
pub mod host_deployer;
pub mod model;
pub mod network_policy;
pub mod resource_monitor;
pub mod sandbox_reaper;
pub mod startup;
pub mod transfer_executor;
pub mod vm_monitor;

use sqlx::PgPool;
use std::sync::Arc;

use crate::configuration::{SchedulingSettings, VmDefaultsSettings};

#[cfg(feature = "otel")]
use common::metrics::Metrics;

#[derive(Clone)]
pub struct App {
    pool: Arc<PgPool>,
    vm_defaults: VmDefaultsSettings,
    scheduling: SchedulingSettings,
    control_plane_architecture: Arc<str>,
    #[cfg(feature = "otel")]
    metrics: Arc<Metrics>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("vm_defaults", &self.vm_defaults)
            .field("scheduling", &self.scheduling)
            .field(
                "control_plane_architecture",
                &self.control_plane_architecture,
            )
            .finish()
    }
}

impl App {
    #[cfg(not(feature = "otel"))]
    pub fn new(
        pool: PgPool,
        vm_defaults: VmDefaultsSettings,
        scheduling: SchedulingSettings,
        control_plane_architecture: String,
    ) -> Self {
        Self {
            pool: Arc::new(pool),
            vm_defaults,
            scheduling,
            control_plane_architecture: Arc::from(control_plane_architecture),
        }
    }

    #[cfg(feature = "otel")]
    pub fn new(
        pool: PgPool,
        vm_defaults: VmDefaultsSettings,
        scheduling: SchedulingSettings,
        control_plane_architecture: String,
    ) -> Self {
        let meter = opentelemetry::global::meter("qarax");
        Self {
            pool: Arc::new(pool),
            vm_defaults,
            scheduling,
            control_plane_architecture: Arc::from(control_plane_architecture),
            metrics: Arc::new(Metrics::new(&meter)),
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn pool_arc(&self) -> Arc<PgPool> {
        self.pool.clone()
    }

    pub fn vm_defaults(&self) -> &VmDefaultsSettings {
        &self.vm_defaults
    }

    pub fn scheduling(&self) -> &SchedulingSettings {
        &self.scheduling
    }

    pub fn control_plane_architecture(&self) -> &str {
        &self.control_plane_architecture
    }

    #[cfg(feature = "otel")]
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    #[cfg(feature = "otel")]
    pub fn metrics_arc(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }
}
