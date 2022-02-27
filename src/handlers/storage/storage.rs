use uuid::Uuid;

use crate::models::{
    storage::{NewStorage, StorageError},
    volumes::{NewVolume, Volume, VolumeError},
};

#[async_trait::async_trait]
pub trait Storage {
    type Persistence;
    type RpcClient;

    fn id(&self) -> Uuid;

    async fn create(
        client: Self::RpcClient,
        p: Self::Persistence,
        new_storage: NewStorage,
    ) -> Result<Self, StorageError>
    where
        Self: Sized;

    async fn create_volume(&self, new_volume: NewVolume) -> Result<Volume, VolumeError>;

    async fn list_volumes(&self) -> Result<(), StorageError>;
}
