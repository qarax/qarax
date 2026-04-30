use uuid::Uuid;

use crate::client::Client;

use super::models::{
    DeployHostRequest, Host, HostEvacuateResponse, HostGpu, HostResourceCapacity, NewHost,
    UpdateHostPlacementRequest, UpdateHostRequest,
};

pub async fn list(
    client: &Client,
    name: Option<&str>,
    architecture: Option<&str>,
) -> anyhow::Result<Vec<Host>> {
    let mut params = vec![];
    if let Some(name) = name {
        params.push(format!("name={}", urlencoding::encode(name)));
    }
    if let Some(architecture) = architecture {
        params.push(format!(
            "architecture={}",
            urlencoding::encode(architecture)
        ));
    }
    let path = if params.is_empty() {
        "/hosts".to_string()
    } else {
        format!("/hosts?{}", params.join("&"))
    };
    client.get(&path).await
}

/// Get a single host by name or UUID string.
pub async fn get(client: &Client, name_or_id: &str) -> anyhow::Result<Host> {
    if let Ok(id) = uuid::Uuid::parse_str(name_or_id) {
        let hosts = list(client, None, None).await?;
        return hosts
            .into_iter()
            .find(|h| h.id == id)
            .ok_or_else(|| anyhow::anyhow!("no host with id {:?}", id));
    }
    let hosts = list(client, Some(name_or_id), None).await?;
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

pub async fn update(client: &Client, host_id: Uuid, req: &UpdateHostRequest) -> anyhow::Result<()> {
    let _: serde_json::Value = client.patch(&format!("/hosts/{host_id}"), req).await?;
    Ok(())
}

pub async fn update_placement(
    client: &Client,
    host_id: Uuid,
    req: &UpdateHostPlacementRequest,
) -> anyhow::Result<()> {
    let _: serde_json::Value = client
        .put(&format!("/hosts/{host_id}/placement"), req)
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

/// Trigger a node upgrade using the last deployed image and stored credentials (async, returns 202).
pub async fn upgrade(client: &Client, host_id: Uuid) -> anyhow::Result<()> {
    client
        .post_empty(&format!("/hosts/{host_id}/upgrade"))
        .await
}

pub async fn evacuate(client: &Client, host_id: Uuid) -> anyhow::Result<HostEvacuateResponse> {
    client
        .post_empty_json(&format!("/hosts/{host_id}/evacuate"))
        .await
}

/// List all GPUs on a host.
pub async fn list_gpus(client: &Client, host_id: Uuid) -> anyhow::Result<Vec<HostGpu>> {
    client.get(&format!("/hosts/{host_id}/gpus")).await
}

pub async fn resources(client: &Client, host_id: Uuid) -> anyhow::Result<HostResourceCapacity> {
    client.get(&format!("/hosts/{host_id}/resources")).await
}
