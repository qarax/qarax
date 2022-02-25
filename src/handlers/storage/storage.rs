use uuid::Uuid;

use crate::{
    handlers::rpc::client::StorageClient,
    models::storage::{NewStorage, StorageError},
};

#[async_trait::async_trait]
pub trait Storage {
    async fn create(
        &self,
        client: &StorageClient,
        new_storage: NewStorage,
    ) -> Result<Uuid, StorageError>
    where
        Self: Sized;

    async fn create_volume(&self) -> Result<(), StorageError>;

    async fn list_volumes(&self) -> Result<(), StorageError>;
}
