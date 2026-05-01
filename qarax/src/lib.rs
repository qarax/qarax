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
pub mod sandbox_pool_manager;
pub mod sandbox_reaper;
pub mod sandbox_runtime;
pub mod startup;
pub mod transfer_executor;
pub mod vm_monitor;

use sqlx::PgPool;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::configuration::{DatabaseSettings, SchedulingSettings, VmDefaultsSettings};

#[cfg(feature = "otel")]
use common::metrics::Metrics;

#[derive(Clone)]
pub struct App {
    pool: Arc<PgPool>,
    database: DatabaseSettings,
    vm_defaults: VmDefaultsSettings,
    scheduling: SchedulingSettings,
    control_plane_architecture: Arc<str>,
    maintenance_mode: Arc<AtomicBool>,
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
        database: DatabaseSettings,
        vm_defaults: VmDefaultsSettings,
        scheduling: SchedulingSettings,
        control_plane_architecture: String,
    ) -> Self {
        Self {
            pool: Arc::new(pool),
            database,
            vm_defaults,
            scheduling,
            control_plane_architecture: Arc::from(control_plane_architecture),
            maintenance_mode: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(feature = "otel")]
    pub fn new(
        pool: PgPool,
        database: DatabaseSettings,
        vm_defaults: VmDefaultsSettings,
        scheduling: SchedulingSettings,
        control_plane_architecture: String,
    ) -> Self {
        let meter = opentelemetry::global::meter("qarax");
        Self {
            pool: Arc::new(pool),
            database,
            vm_defaults,
            scheduling,
            control_plane_architecture: Arc::from(control_plane_architecture),
            maintenance_mode: Arc::new(AtomicBool::new(false)),
            metrics: Arc::new(Metrics::new(&meter)),
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn pool_arc(&self) -> Arc<PgPool> {
        self.pool.clone()
    }

    pub fn database(&self) -> &DatabaseSettings {
        &self.database
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

    pub fn maintenance_mode(&self) -> bool {
        self.maintenance_mode.load(Ordering::SeqCst)
    }

    pub fn set_maintenance_mode(&self, enabled: bool) {
        self.maintenance_mode.store(enabled, Ordering::SeqCst);
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
