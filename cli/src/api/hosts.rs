use uuid::Uuid;

use crate::client::Client;

use super::models::{DeployHostRequest, Host, HostGpu, NewHost};

pub async fn list(client: &Client) -> anyhow::Result<Vec<Host>> {
    client.get("/hosts").await
}

/// Add a new host. Returns the new host's UUID as a plain-text string.
pub async fn add(client: &Client, host: &NewHost) -> anyhow::Result<String> {
    client.post_text("/hosts", host).await
}

/// Deploy a bootc image to a host (async, returns 202).
pub async fn deploy(client: &Client, host_id: Uuid, req: &DeployHostRequest) -> anyhow::Result<()> {
    client
        .post_text(&format!("/hosts/{host_id}/deploy"), req)
        .await?;
    Ok(())
}

/// Initialize a host (connect via gRPC, populate version info, mark UP).
/// Returns the updated Host.
pub async fn init(client: &Client, host_id: Uuid) -> anyhow::Result<Host> {
    client
        .post_empty_json(&format!("/hosts/{host_id}/init"))
        .await
}

/// List all GPUs on a host.
pub async fn list_gpus(client: &Client, host_id: Uuid) -> anyhow::Result<Vec<HostGpu>> {
    client.get(&format!("/hosts/{host_id}/gpus")).await
}
