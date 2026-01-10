pub mod configuration;
pub mod database;
pub mod errors;
pub mod handlers;
pub mod model;
pub mod startup;

use sqlx::PgPool;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct App {
    pool: Arc<PgPool>,
}

impl App {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
