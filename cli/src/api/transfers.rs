use uuid::Uuid;

use crate::client::Client;

use super::models::{NewTransfer, Transfer};

pub async fn list(
    client: &Client,
    pool_id: Uuid,
    name: Option<&str>,
) -> anyhow::Result<Vec<Transfer>> {
    let path = match name {
        Some(n) => format!("/storage-pools/{pool_id}/transfers?name={n}"),
        None => format!("/storage-pools/{pool_id}/transfers"),
    };
    client.get(&path).await
}

pub async fn get(client: &Client, pool_id: Uuid, transfer_id: Uuid) -> anyhow::Result<Transfer> {
    client
        .get(&format!("/storage-pools/{pool_id}/transfers/{transfer_id}"))
        .await
}

/// Starts a transfer. Returns the created `Transfer` object (202 Accepted).
pub async fn create(
    client: &Client,
    pool_id: Uuid,
    transfer: &NewTransfer,
) -> anyhow::Result<Transfer> {
    client
        .post(&format!("/storage-pools/{pool_id}/transfers"), transfer)
        .await
}
