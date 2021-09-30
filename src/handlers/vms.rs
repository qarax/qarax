use super::*;

use crate::handlers::models::drives::{CacheType, Drive, Status as DriveStatus};
use crate::handlers::rpc::node::{Drive as VmDrive, VmConfig};
use axum::extract::{Json, Path};
use models::vms as vm_model;
use models::vms::{NewVm, Vm};

pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<Vm>>, ServerError> {
    let vms = vm_model::list(env.db()).await.map_err(|e| {
        tracing::error!("Failed to list vms, error: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        data: vms,
        code: StatusCode::OK,
    })
}

pub async fn add(
    Extension(env): Extension<Environment>,
    Json(vm): Json<NewVm>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let vm_id = vm_model::add(env.db(), &vm).await.map_err(|e| {
        tracing::error!("Can't add vm: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        data: vm_id,
        code: StatusCode::CREATED,
    })
}

pub async fn get(
    Extension(env): Extension<Environment>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<Vm>, ServerError> {
    let vm = vm_model::by_id(env.db(), &vm_id).await.map_err(|e| {
        tracing::error!("Can't find vm, error: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        data: vm,
        code: StatusCode::CREATED,
    })
}

pub async fn start(
    Extension(env): Extension<Environment>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<String>, ServerError> {
    let host = hosts::find_running_host(env.db()).await?;
    tracing::debug!("found host {:?}", host);
    let clients = &*env.clients().read().await;

    let client = clients.get(&host.id).unwrap();
    let vm = vm_model::by_id(env.db(), &vm_id).await.map_err(|e| {
        tracing::error!("Can't find vm, error: {}", e);
        ServerError::Internal
    })?;

    let request = VmConfig {
        vm_id: vm.id.to_string(),
        memory: vm.memory,
        vcpus: vm.vcpu,
        kernel: String::from("/root/vmlinux.bin"), // TODO super temporary
        kernel_params: String::from("console=ttyS0 reboot=k panic=1 pci=off"), // TODO super temporary
        network_mode: String::new(),
        ip_address: String::new(),
        mac_address: String::new(),
        drives: vec![VmDrive {
            drive_id: String::from("1"),
            is_read_only: false,
            is_root_device: true,
            path_on_host: String::from("/root/bionic.rootfs.ext4"), // TODO super temporary
            cache_type: CacheType::None as i32,
        }],
    };

    tracing::info!("Starting VM {} with config: {:?}", vm.id, request);

    let response = client.start_vm(request).await.map_err(|e| {
        tracing::error!("Can't start vm, error: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        code: StatusCode::ACCEPTED,
        data: response.into_inner().vm_id,
    })
}
