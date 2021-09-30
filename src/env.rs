use sqlx::postgres::PgPool;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::handlers::rpc::client::Client;

#[derive(Clone, Debug)]
pub struct Environment {
    pool: PgPool,
    clients: Arc<RwLock<HashMap<Uuid, Client>>>,
}

impl Environment {
    pub async fn new(pool: PgPool) -> anyhow::Result<Self> {
        Ok(Self {
            pool,
            clients: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn db(&self) -> &PgPool {
        &self.pool
    }

    pub fn clients(&self) -> &RwLock<HashMap<Uuid, Client>> {
        &self.clients
    }
}
