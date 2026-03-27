use uuid::Uuid;

use crate::client::Client;

use super::models::{CreateSandboxResponse, NewSandbox, Sandbox};

pub async fn create(client: &Client, req: &NewSandbox) -> anyhow::Result<CreateSandboxResponse> {
    client.post("/sandboxes", req).await
}

pub async fn list(client: &Client) -> anyhow::Result<Vec<Sandbox>> {
    client.get("/sandboxes").await
}

pub async fn get(client: &Client, id: Uuid) -> anyhow::Result<Sandbox> {
    client.get(&format!("/sandboxes/{id}")).await
}

pub async fn delete(client: &Client, id: Uuid) -> anyhow::Result<()> {
    client.delete(&format!("/sandboxes/{id}")).await
}
