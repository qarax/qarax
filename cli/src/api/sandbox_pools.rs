use uuid::Uuid;

use crate::client::Client;

use super::models::{ConfigureSandboxPoolRequest, SandboxPool};

pub async fn list(client: &Client) -> anyhow::Result<Vec<SandboxPool>> {
    client.get("/sandbox-pools").await
}

pub async fn get(client: &Client, vm_template_id: Uuid) -> anyhow::Result<SandboxPool> {
    client
        .get(&format!("/vm-templates/{vm_template_id}/sandbox-pool"))
        .await
}

pub async fn put(
    client: &Client,
    vm_template_id: Uuid,
    request: &ConfigureSandboxPoolRequest,
) -> anyhow::Result<SandboxPool> {
    client
        .put(
            &format!("/vm-templates/{vm_template_id}/sandbox-pool"),
            request,
        )
        .await
}

pub async fn delete(client: &Client, vm_template_id: Uuid) -> anyhow::Result<()> {
    client
        .delete(&format!("/vm-templates/{vm_template_id}/sandbox-pool"))
        .await
}
