pub mod configuration;
pub mod database;
pub mod errors;
pub mod grpc_client;
pub mod handlers;
pub mod model;
pub mod startup;

use sqlx::PgPool;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct App {
    pool: Arc<PgPool>,
    qarax_node_address: String,
}

impl App {
    pub fn new(pool: PgPool, qarax_node_address: String) -> Self {
        Self {
            pool: Arc::new(pool),
            qarax_node_address,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn qarax_node_address(&self) -> &str {
        &self.qarax_node_address
    }
}
