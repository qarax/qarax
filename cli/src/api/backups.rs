use uuid::Uuid;

use crate::client::Client;

use super::models::{Backup, CreateBackupRequest, RestoreBackupResponse};

pub async fn list(
    client: &Client,
    name: Option<&str>,
    backup_type: Option<&str>,
) -> anyhow::Result<Vec<Backup>> {
    let mut params = vec![];
    if let Some(name) = name {
        params.push(format!("name={}", urlencoding::encode(name)));
    }
    if let Some(backup_type) = backup_type {
        params.push(format!("backup_type={}", urlencoding::encode(backup_type)));
    }
    let path = if params.is_empty() {
        "/backups".to_string()
    } else {
        format!("/backups?{}", params.join("&"))
    };
    client.get(&path).await
}

pub async fn get(client: &Client, backup_id: Uuid) -> anyhow::Result<Backup> {
    client.get(&format!("/backups/{backup_id}")).await
}

pub async fn create(client: &Client, request: &CreateBackupRequest) -> anyhow::Result<Backup> {
    client.post("/backups", request).await
}

pub async fn restore(client: &Client, backup_id: Uuid) -> anyhow::Result<RestoreBackupResponse> {
    client
        .post_empty_json(&format!("/backups/{backup_id}/restore"))
        .await
}
