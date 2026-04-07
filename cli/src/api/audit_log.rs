use uuid::Uuid;

use crate::client::Client;

use super::models::AuditLog;

pub async fn list(
    client: &Client,
    resource_type: Option<&str>,
    resource_id: Option<Uuid>,
    action: Option<&str>,
    limit: Option<i64>,
) -> anyhow::Result<Vec<AuditLog>> {
    let mut params = vec![];
    if let Some(rt) = resource_type {
        params.push(format!("resource_type={}", urlencoding::encode(rt)));
    }
    if let Some(rid) = resource_id {
        params.push(format!("resource_id={rid}"));
    }
    if let Some(a) = action {
        params.push(format!("action={}", urlencoding::encode(a)));
    }
    if let Some(l) = limit {
        params.push(format!("limit={l}"));
    }

    let path = if params.is_empty() {
        "/audit-logs".to_string()
    } else {
        format!("/audit-logs?{}", params.join("&"))
    };

    client.get(&path).await
}

pub async fn get(client: &Client, id: Uuid) -> anyhow::Result<AuditLog> {
    client.get(&format!("/audit-logs/{id}")).await
}
