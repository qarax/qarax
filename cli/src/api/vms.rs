use anyhow::{Context, anyhow};
use reqwest::StatusCode;
use uuid::Uuid;

use crate::client::Client;

use super::models::{
    AttachDiskRequest, CommitVmRequest, CommitVmResponse, CreateSnapshotRequest, CreateVmResponse,
    CreateVmResult, DiskResizeRequest, HotplugNicRequest, NetworkInterface, NewVm, RestoreRequest,
    SecurityGroup, Snapshot, StorageObject, Vm, VmDisk, VmImagePreflightRequest,
    VmImagePreflightResponse, VmMigrateRequest, VmMigrateResponse, VmResizeRequest,
    VmStartResponse,
};

pub async fn list(client: &Client, name: Option<&str>, tags: &[String]) -> anyhow::Result<Vec<Vm>> {
    let mut params = vec![];
    if let Some(n) = name {
        params.push(format!("name={}", urlencoding::encode(n)));
    }
    if !tags.is_empty() {
        params.push(format!("tags={}", urlencoding::encode(&tags.join(","))));
    }
    let path = if params.is_empty() {
        "/vms".to_string()
    } else {
        format!("/vms?{}", params.join("&"))
    };
    client.get(&path).await
}

pub async fn get(client: &Client, vm_id: Uuid) -> anyhow::Result<Vm> {
    client.get(&format!("/vms/{vm_id}")).await
}

/// Creates a VM. The server returns 201 (sync, body is a JSON-quoted UUID string)
/// or 202 (async with image pull, body is `CreateVmResponse`).
pub async fn create(client: &Client, vm: &NewVm) -> anyhow::Result<CreateVmResult> {
    let resp = client.post_response("/vms", vm).await?;
    match resp.status() {
        StatusCode::CREATED => {
            let id_str: String = resp.json().await.context("failed to parse VM id")?;
            let vm_id =
                Uuid::parse_str(&id_str).with_context(|| format!("invalid UUID: {id_str}"))?;
            Ok(CreateVmResult::Created(vm_id))
        }
        StatusCode::ACCEPTED => {
            let body: CreateVmResponse = resp
                .json()
                .await
                .context("failed to parse CreateVmResponse")?;
            Ok(CreateVmResult::Accepted {
                vm_id: body.vm_id,
                job_id: body.job_id,
            })
        }
        status => Err(anyhow!("unexpected status: {status}")),
    }
}

pub async fn delete(client: &Client, vm_id: Uuid) -> anyhow::Result<()> {
    client.delete(&format!("/vms/{vm_id}")).await
}

pub async fn start(client: &Client, vm_id: Uuid) -> anyhow::Result<VmStartResponse> {
    client.post_empty_json(&format!("/vms/{vm_id}/start")).await
}

pub async fn preflight_image(
    client: &Client,
    req: &VmImagePreflightRequest,
) -> anyhow::Result<VmImagePreflightResponse> {
    client.post("/vms/preflight", req).await
}

pub async fn stop(client: &Client, vm_id: Uuid) -> anyhow::Result<()> {
    client.post_empty(&format!("/vms/{vm_id}/stop")).await
}

pub async fn force_stop(client: &Client, vm_id: Uuid) -> anyhow::Result<()> {
    client.post_empty(&format!("/vms/{vm_id}/force-stop")).await
}

pub async fn pause(client: &Client, vm_id: Uuid) -> anyhow::Result<()> {
    client.post_empty(&format!("/vms/{vm_id}/pause")).await
}

pub async fn resume(client: &Client, vm_id: Uuid) -> anyhow::Result<()> {
    client.post_empty(&format!("/vms/{vm_id}/resume")).await
}

pub async fn console_log(client: &Client, vm_id: Uuid) -> anyhow::Result<String> {
    client.get_text(&format!("/vms/{vm_id}/console")).await
}

pub async fn attach_disk(
    client: &Client,
    vm_id: Uuid,
    req: &AttachDiskRequest,
) -> anyhow::Result<VmDisk> {
    client.post(&format!("/vms/{vm_id}/disks"), req).await
}

pub async fn remove_disk(client: &Client, vm_id: Uuid, device_id: &str) -> anyhow::Result<()> {
    client
        .delete(&format!("/vms/{vm_id}/disks/{device_id}"))
        .await
}

pub async fn add_nic(
    client: &Client,
    vm_id: Uuid,
    req: &HotplugNicRequest,
) -> anyhow::Result<NetworkInterface> {
    client.post(&format!("/vms/{vm_id}/nics"), req).await
}

pub async fn remove_nic(client: &Client, vm_id: Uuid, device_id: &str) -> anyhow::Result<()> {
    client
        .delete(&format!("/vms/{vm_id}/nics/{device_id}"))
        .await
}

pub async fn list_nics(client: &Client, vm_id: Uuid) -> anyhow::Result<Vec<NetworkInterface>> {
    client.get(&format!("/vms/{vm_id}/nics")).await
}

pub async fn list_security_groups(
    client: &Client,
    vm_id: Uuid,
) -> anyhow::Result<Vec<SecurityGroup>> {
    client.get(&format!("/vms/{vm_id}/security-groups")).await
}

pub async fn create_snapshot(
    client: &Client,
    vm_id: Uuid,
    req: &CreateSnapshotRequest,
) -> anyhow::Result<Snapshot> {
    client.post(&format!("/vms/{vm_id}/snapshots"), req).await
}

pub async fn list_snapshots(
    client: &Client,
    vm_id: Uuid,
    name: Option<&str>,
) -> anyhow::Result<Vec<Snapshot>> {
    let path = match name {
        Some(n) => format!("/vms/{vm_id}/snapshots?name={n}"),
        None => format!("/vms/{vm_id}/snapshots"),
    };
    client.get(&path).await
}

pub async fn restore(client: &Client, vm_id: Uuid, req: &RestoreRequest) -> anyhow::Result<Vm> {
    client.post(&format!("/vms/{vm_id}/restore"), req).await
}

pub async fn migrate(
    client: &Client,
    vm_id: Uuid,
    req: &VmMigrateRequest,
) -> anyhow::Result<VmMigrateResponse> {
    client.post(&format!("/vms/{vm_id}/migrate"), req).await
}

pub async fn resize(client: &Client, vm_id: Uuid, req: &VmResizeRequest) -> anyhow::Result<Vm> {
    client.put(&format!("/vms/{vm_id}/resize"), req).await
}

pub async fn resize_disk(
    client: &Client,
    vm_id: Uuid,
    disk_id: &str,
    req: &DiskResizeRequest,
) -> anyhow::Result<StorageObject> {
    client
        .put(&format!("/vms/{vm_id}/disks/{disk_id}/resize"), req)
        .await
}

pub async fn commit(
    client: &Client,
    vm_id: Uuid,
    req: &CommitVmRequest,
) -> anyhow::Result<CommitVmResponse> {
    client.post(&format!("/vms/{vm_id}/commit"), req).await
}
