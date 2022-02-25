use http::HeaderValue;
use sqlx::PgPool;
use uuid::Uuid;

use super::storage::Storage;
use crate::models::storage::{self as storage_model, NewStorage};
use crate::{
    handlers::rpc::client::{StorageClient, StorageCreateRequest},
    models::storage::StorageConfig,
};

#[derive(Debug)]
pub struct LocalStorage {
    name: String,
    config: StorageConfig,
    pool: PgPool,
}

impl LocalStorage {
    pub fn new(name: String, config: StorageConfig, pool: PgPool) -> Self {
        Self { name, config, pool }
    }
}

#[async_trait::async_trait]
impl Storage for LocalStorage {
    #[tracing::instrument]
    async fn create(
        &self,
        client: &StorageClient,
        new_storage: NewStorage,
    ) -> Result<Uuid, crate::models::storage::StorageError>
    where
        Self: Sized,
    {
        let current = tracing::span::Span::current();
        tracing::info!("request_id = {:?}", current.metadata().unwrap().fields());
        let storage_id = storage_model::add(&self.pool, &new_storage).await?;
        let storage = storage_model::by_id(&self.pool, storage_id).await?;
        let request = StorageCreateRequest {
            storage,
            request_id: "create".to_owned(),
        };

        client.create(request).await.unwrap();
        Ok(storage_id)
    }

    #[tracing::instrument]
    async fn create_volume(&self) -> Result<(), crate::models::storage::StorageError> {
        todo!()
    }

    #[tracing::instrument]
    async fn list_volumes(&self) -> Result<(), crate::models::storage::StorageError> {
        todo!()
    }
}
