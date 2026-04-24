use uuid::Uuid;

use crate::client::Client;

use super::models::{
    AttachHostToPoolRequest, CreateDiskRequest, CreateDiskResponse, ImportToPoolRequest,
    ImportToPoolResponse, NewStorageObject, NewStoragePool, RegisterLunRequest, StorageObject,
    StoragePool,
};

// Storage pools

pub async fn list_pools(client: &Client, name: Option<&str>) -> anyhow::Result<Vec<StoragePool>> {
    let path = match name {
        Some(n) => format!("/storage-pools?name={n}"),
        None => "/storage-pools".to_string(),
    };
    client.get(&path).await
}

pub async fn get_pool(client: &Client, pool_id: Uuid) -> anyhow::Result<StoragePool> {
    client.get(&format!("/storage-pools/{pool_id}")).await
}

/// Create a storage pool. Returns the new pool's UUID as plain text.
pub async fn create_pool(client: &Client, pool: &NewStoragePool) -> anyhow::Result<String> {
    client.post_text("/storage-pools", pool).await
}

pub async fn delete_pool(client: &Client, pool_id: Uuid) -> anyhow::Result<()> {
    client.delete(&format!("/storage-pools/{pool_id}")).await
}

pub async fn attach_host_to_pool(
    client: &Client,
    pool_id: Uuid,
    host_id: Uuid,
) -> anyhow::Result<()> {
    client
        .post_response(
            &format!("/storage-pools/{pool_id}/hosts"),
            &AttachHostToPoolRequest { host_id },
        )
        .await?;
    Ok(())
}

pub async fn detach_host_from_pool(
    client: &Client,
    pool_id: Uuid,
    host_id: Uuid,
) -> anyhow::Result<()> {
    client
        .delete(&format!("/storage-pools/{pool_id}/hosts/{host_id}"))
        .await
}

// Storage objects

pub async fn list_objects(
    client: &Client,
    name: Option<&str>,
    pool_id: Option<Uuid>,
    object_type: Option<&str>,
) -> anyhow::Result<Vec<StorageObject>> {
    let mut params: Vec<String> = Vec::new();
    if let Some(n) = name {
        params.push(format!("name={}", urlencoding::encode(n)));
    }
    if let Some(id) = pool_id {
        params.push(format!("pool_id={id}"));
    }
    if let Some(t) = object_type {
        params.push(format!("object_type={}", urlencoding::encode(t)));
    }
    let path = if params.is_empty() {
        "/storage-objects".to_string()
    } else {
        format!("/storage-objects?{}", params.join("&"))
    };
    client.get(&path).await
}

pub async fn get_object(client: &Client, object_id: Uuid) -> anyhow::Result<StorageObject> {
    client.get(&format!("/storage-objects/{object_id}")).await
}

/// Create a storage object. Returns the new object's UUID as plain text.
pub async fn create_object(client: &Client, obj: &NewStorageObject) -> anyhow::Result<String> {
    client.post_text("/storage-objects", obj).await
}

pub async fn delete_object(client: &Client, object_id: Uuid) -> anyhow::Result<()> {
    client
        .delete(&format!("/storage-objects/{object_id}"))
        .await
}

pub async fn create_disk(
    client: &Client,
    pool_id: Uuid,
    req: &CreateDiskRequest,
) -> anyhow::Result<CreateDiskResponse> {
    client
        .post(&format!("/storage-pools/{pool_id}/disks"), req)
        .await
}

pub async fn import_to_pool(
    client: &Client,
    pool_id: Uuid,
    req: &ImportToPoolRequest,
) -> anyhow::Result<ImportToPoolResponse> {
    client
        .post(&format!("/storage-pools/{pool_id}/import"), req)
        .await
}

pub async fn register_lun(
    client: &Client,
    pool_id: Uuid,
    req: &RegisterLunRequest,
) -> anyhow::Result<CreateDiskResponse> {
    client
        .post(&format!("/storage-pools/{pool_id}/luns"), req)
        .await
}
