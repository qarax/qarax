use uuid::Uuid;

use crate::client::Client;

use super::models::{CreateVmTemplateFromVmRequest, NewVmTemplate, VmTemplate};

pub async fn list(client: &Client) -> anyhow::Result<Vec<VmTemplate>> {
    client.get("/vm-templates").await
}

pub async fn get(client: &Client, id: Uuid) -> anyhow::Result<VmTemplate> {
    client.get(&format!("/vm-templates/{id}")).await
}

pub async fn create(client: &Client, vm_template: &NewVmTemplate) -> anyhow::Result<String> {
    client.post_text("/vm-templates", vm_template).await
}

pub async fn create_from_vm(
    client: &Client,
    vm_id: Uuid,
    request: &CreateVmTemplateFromVmRequest,
) -> anyhow::Result<String> {
    client
        .post_text(&format!("/vms/{vm_id}/template"), request)
        .await
}

pub async fn delete(client: &Client, id: Uuid) -> anyhow::Result<()> {
    client.delete(&format!("/vm-templates/{id}")).await
}
