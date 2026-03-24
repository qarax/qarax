use uuid::Uuid;

use crate::client::Client;

use super::models::{DeployHostRequest, Host, HostGpu, NewHost};

pub async fn list(client: &Client, name: Option<&str>) -> anyhow::Result<Vec<Host>> {
    let path = match name {
        Some(n) => format!("/hosts?name={n}"),
        None => "/hosts".to_string(),
    };
    client.get(&path).await
}

/// Get a single host by name or UUID string.
pub async fn get(client: &Client, name_or_id: &str) -> anyhow::Result<Host> {
    if let Ok(id) = uuid::Uuid::parse_str(name_or_id) {
        let hosts = list(client, None).await?;
        return hosts
            .into_iter()
            .find(|h| h.id == id)
            .ok_or_else(|| anyhow::anyhow!("no host with id {:?}", id));
    }
    let hosts = list(client, Some(name_or_id)).await?;
    hosts
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no host named {:?}", name_or_id))
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
