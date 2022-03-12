use http::header::CONTENT_LENGTH;
use http::HeaderValue;
use sqlx::PgPool;
use uuid::Uuid;

use super::storage::Storage;
use crate::handlers::rpc::client::{StorageClient, StorageCreateRequest, VolumeCreateRequest};
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
        let storage = storage_model::by_id(&pool, &storage_id).await?;
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
        if let Some(url) = &new_volume.url {
            let http_client = reqwest::Client::new();
            let response = http_client.head(url).send().await.unwrap();

            let headers = response.headers();
            if headers.contains_key(CONTENT_LENGTH) {
                let size = HeaderValue::to_str(&headers[CONTENT_LENGTH]).unwrap();
                let volume_id = volume_model::add(
                    &self.pool,
                    &new_volume,
                    self.id(),
                    size.parse::<i64>().unwrap(),
                )
                .await?;
                let volume = volume_model::by_id(&self.pool, volume_id).await?;
                self.client
                    .create_volume(VolumeCreateRequest {
                        storage: self.storage.clone(),
                        volume: volume.clone(),
                        request_id: "create_volume".to_owned(),
                        url: url.to_string(),
                    })
                    .await
                    .unwrap();

                return Ok(volume);
            }
        }

        Err(VolumeError::Other(
            "url is not valid (TODO: support creation of empty volumes)".into(),
        ))
    }

    #[tracing::instrument]
    async fn list_volumes(&self) -> Result<(), crate::models::storage::StorageError> {
        todo!()
    }
}
