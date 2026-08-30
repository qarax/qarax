pub mod auth;
pub mod configuration;
pub mod database;
pub mod errors;
pub mod grpc_client;
pub mod ha_monitor;
pub mod handlers;
pub mod hook_executor;
pub mod host_deployer;
pub mod model;
pub mod network_policy;
pub mod resource_monitor;
pub mod sandbox_pool_manager;
pub mod sandbox_reaper;
pub mod sandbox_runtime;
pub mod secret_provider;
pub mod startup;
pub mod transfer_executor;
pub mod vm_monitor;

use sqlx::PgPool;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::configuration::{
    AuthSettings, DatabaseSettings, SandboxSettings, SchedulingSettings, VmDefaultsSettings,
};
use crate::secret_provider::{ExternalSecretProvider, SecretProvider};

#[cfg(feature = "otel")]
use common::metrics::Metrics;

#[derive(Clone)]
pub struct App {
    pool: Arc<PgPool>,
    database: DatabaseSettings,
    vm_defaults: VmDefaultsSettings,
    scheduling: SchedulingSettings,
    sandbox: SandboxSettings,
    auth: AuthSettings,
    control_plane_architecture: Arc<str>,
    maintenance_mode: Arc<AtomicBool>,
    secret_provider: Arc<dyn SecretProvider>,
    #[cfg(feature = "otel")]
    metrics: Arc<Metrics>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("vm_defaults", &self.vm_defaults)
            .field("scheduling", &self.scheduling)
            .field("sandbox", &self.sandbox)
            .field(
                "control_plane_architecture",
                &self.control_plane_architecture,
            )
            .finish()
    }
}

impl App {
    #[cfg(not(feature = "otel"))]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: PgPool,
        database: DatabaseSettings,
        vm_defaults: VmDefaultsSettings,
        scheduling: SchedulingSettings,
        sandbox: SandboxSettings,
        auth: AuthSettings,
        control_plane_architecture: String,
    ) -> Self {
        Self {
            pool: Arc::new(pool),
            database,
            vm_defaults,
            scheduling,
            sandbox,
            auth,
            control_plane_architecture: Arc::from(control_plane_architecture),
            maintenance_mode: Arc::new(AtomicBool::new(false)),
            secret_provider: Arc::new(ExternalSecretProvider),
        }
    }

    #[cfg(feature = "otel")]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: PgPool,
        database: DatabaseSettings,
        vm_defaults: VmDefaultsSettings,
        scheduling: SchedulingSettings,
        sandbox: SandboxSettings,
        auth: AuthSettings,
        control_plane_architecture: String,
    ) -> Self {
        let meter = opentelemetry::global::meter("qarax");
        Self {
            pool: Arc::new(pool),
            database,
            vm_defaults,
            scheduling,
            sandbox,
            auth,
            control_plane_architecture: Arc::from(control_plane_architecture),
            maintenance_mode: Arc::new(AtomicBool::new(false)),
            secret_provider: Arc::new(ExternalSecretProvider),
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

    pub fn sandbox(&self) -> &SandboxSettings {
        &self.sandbox
    }

    pub fn auth(&self) -> &AuthSettings {
        &self.auth
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

    pub fn secret_provider(&self) -> &dyn SecretProvider {
        self.secret_provider.as_ref()
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
