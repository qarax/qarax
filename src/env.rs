use sqlx::postgres::PgPool;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::handlers::rpc::client::{StorageClient, VmmClient};

#[derive(Clone, Debug)]
pub struct Environment {
    pool: Arc<PgPool>,
    vmm_clients: Arc<RwLock<HashMap<Uuid, VmmClient>>>,
    storage_clients: Arc<RwLock<HashMap<Uuid, StorageClient>>>,
}

impl Environment {
    pub async fn new(pool: PgPool) -> anyhow::Result<Self> {
        Ok(Self {
            pool: Arc::new(pool),
            vmm_clients: Arc::new(RwLock::new(HashMap::new())),
            storage_clients: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn db(&self) -> &PgPool {
        &self.pool
    }

    pub fn vmm_clients(&self) -> &RwLock<HashMap<Uuid, VmmClient>> {
        &self.vmm_clients
    }

    pub fn storage_clients(&self) -> &RwLock<HashMap<Uuid, StorageClient>> {
        &self.storage_clients
    }
}
