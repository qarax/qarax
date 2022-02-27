use sqlx::PgPool;
use uuid::Uuid;

use super::storage::Storage;
use crate::handlers::rpc::client::{StorageClient, StorageCreateRequest};
use crate::models::storage::{self as storage_model, NewStorage};
use crate::models::volumes::{self as volume_model, NewVolume, Volume, VolumeError};

#[derive(Debug)]
pub struct LocalStorage {
    storage: storage_model::Storage,
    client: StorageClient,
    pool: PgPool,
}

impl LocalStorage {
    pub fn new(storage: storage_model::Storage, client: StorageClient, pool: PgPool) -> Self {
        Self {
            storage,
            client,
            pool,
        }
    }
}

#[async_trait::async_trait]
impl Storage for LocalStorage {
    type Persistence = PgPool;
    type RpcClient = StorageClient;

    fn id(&self) -> Uuid {
        self.storage.id
    }

    #[tracing::instrument]
    async fn create(
        client: Self::RpcClient,
        pool: Self::Persistence,
        new_storage: NewStorage,
    ) -> Result<Self, crate::models::storage::StorageError>
    where
        Self: Sized,
    {
        let storage_id = storage_model::add(&pool, &new_storage).await?;
        let storage = storage_model::by_id(&pool, storage_id).await?;
        let request = StorageCreateRequest {
            storage: storage.clone(),
            request_id: "create".to_owned(),
        };

        // TODO handle errors
        client.create(request).await.unwrap();

        Ok(Self {
            storage,
            client,
            pool,
        })
    }

    #[tracing::instrument]
    async fn create_volume(&self, new_volume: NewVolume) -> Result<Volume, VolumeError> {
        let volume_id = volume_model::add(&self.pool, &new_volume, self.id()).await?;
        let volume = volume_model::by_id(&self.pool, volume_id).await?;
        Ok(volume)
    }

    #[tracing::instrument]
    async fn list_volumes(&self) -> Result<(), crate::models::storage::StorageError> {
        todo!()
    }
}
