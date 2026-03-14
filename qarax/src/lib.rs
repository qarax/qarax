pub mod configuration;
pub mod database;
pub mod errors;
pub mod grpc_client;
pub mod handlers;
pub mod host_deployer;
pub mod model;
pub mod resource_monitor;
pub mod startup;
pub mod transfer_executor;
pub mod vm_monitor;

use sqlx::PgPool;
use std::sync::Arc;

use crate::configuration::VmDefaultsSettings;

#[derive(Debug, Clone)]
pub struct App {
    pool: Arc<PgPool>,
    vm_defaults: VmDefaultsSettings,
    snapshot_dir: String,
}

impl App {
    pub fn new(pool: PgPool, vm_defaults: VmDefaultsSettings) -> Self {
        let snapshot_dir = std::env::var("SNAPSHOT_DIR")
            .unwrap_or_else(|_| "/var/lib/qarax/snapshots".to_string());
        Self {
            pool: Arc::new(pool),
            vm_defaults,
            snapshot_dir,
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

    pub fn snapshot_dir(&self) -> &str {
        &self.snapshot_dir
    }
}
