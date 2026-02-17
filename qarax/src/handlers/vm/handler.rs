use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    App,
    grpc_client::{CreateVmRequest, NodeClient, net_configs_from_api},
    model::{
        boot_sources, hosts,
        vms::{self, NewVm, Vm, VmStatus},
    },
};

use super::{ApiResponse, Result};

/// Returns the host id for the configured qarax-node (address:port) if a host is registered.
async fn node_host_id(env: &App) -> Result<Option<Uuid>, crate::errors::Error> {
    let (address, port_str) = env
        .qarax_node_address()
        .split_once(':')
        .unwrap_or(("", "0"));
    let port: i32 = port_str.parse().unwrap_or(0);
    hosts::id_by_address_and_port(env.pool(), address, port)
        .await
        .map_err(crate::errors::Error::Sqlx)
}

#[utoipa::path(
    get,
    path = "/vms",
    responses(
        (status = 200, description = "List all VMs", body = Vec<Vm>),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<Vm>>> {
    let hosts = vms::list(env.pool()).await?;
    Ok(ApiResponse {
        data: hosts,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/vms/{vm_id}",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM details", body = Vm),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<Vm>> {
    let vm = vms::get(env.pool(), vm_id).await?;
    Ok(ApiResponse {
        data: vm,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/vms",
    request_body = NewVm,
    responses(
        (status = 201, description = "VM created successfully", body = String),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(vm): Json<NewVm>,
) -> Result<(StatusCode, String)> {
    let mut tx = env.pool().begin().await?;
    let id = vms::create_tx(&mut tx, &vm).await?;

    // Resolve boot source or use defaults
    let (kernel, initramfs, cmdline) = if let Some(boot_source_id) = vm.boot_source_id {
        // Resolve boot source
        let resolved = boot_sources::resolve(env.pool(), boot_source_id)
            .await
            .map_err(|e| {
                tracing::error!("Failed to resolve boot_source_id {}: {}", boot_source_id, e);
                crate::errors::Error::UnprocessableEntity(format!(
                    "Invalid boot_source_id: {}",
                    boot_source_id
                ))
            })?;

        (
            resolved.kernel_path,
            resolved.initramfs_path,
            resolved.kernel_params,
        )
    } else {
        // Fall back to vm_defaults
        let vm_defaults = env.vm_defaults();
        (
            vm_defaults.kernel.clone(),
            vm_defaults.initramfs.clone(),
            vm_defaults.cmdline.clone(),
        )
    };

    // Call qarax-node to create the VM; on failure we return before commit so the insert is rolled back
    let networks = net_configs_from_api(vm.networks.as_deref().unwrap_or(&[]));
    let node_client = NodeClient::from_address(env.qarax_node_address());
    if let Err(e) = node_client
        .create_vm(CreateVmRequest {
            vm_id: id,
            boot_vcpus: vm.boot_vcpus,
            max_vcpus: vm.max_vcpus,
            memory_size: vm.memory_size,
            networks,
            kernel,
            initramfs,
            cmdline,
        })
        .await
    {
        tracing::error!("Failed to create VM on qarax-node: {}", e);
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "qarax-node: {}",
            e
        )));
    }

    tx.commit().await?;

    // Set host_id if a host is registered for the node we used
    if let Some(host_id) = node_host_id(&env).await? {
        let _ = vms::update_host_id(env.pool(), id, host_id).await;
    }

    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/start",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM started successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn start(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    // Call qarax-node to start the VM
    let node_client = NodeClient::from_address(env.qarax_node_address());
    node_client.start_vm(vm_id).await.map_err(|e| {
        tracing::error!("Failed to start VM on qarax-node: {}", e);
        crate::errors::Error::InternalServerError
    })?;

    // Update status in database
    vms::update_status(env.pool(), vm_id, VmStatus::Running).await?;

    // Set host_id if still unset and a host is registered for the node we used
    if let Some(host_id) = node_host_id(&env).await? {
        let _ = vms::update_host_id(env.pool(), vm_id, host_id).await;
    }

    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/stop",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM stopped successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn stop(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    // Call qarax-node to stop the VM
    let node_client = NodeClient::from_address(env.qarax_node_address());
    node_client.stop_vm(vm_id).await.map_err(|e| {
        tracing::error!("Failed to stop VM on qarax-node: {}", e);
        crate::errors::Error::InternalServerError
    })?;

    // Update status in database
    vms::update_status(env.pool(), vm_id, VmStatus::Shutdown).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/pause",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM paused successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn pause(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    // Call qarax-node to pause the VM
    let node_client = NodeClient::from_address(env.qarax_node_address());
    node_client.pause_vm(vm_id).await.map_err(|e| {
        tracing::error!("Failed to pause VM on qarax-node: {}", e);
        crate::errors::Error::InternalServerError
    })?;

    // Update status in database
    vms::update_status(env.pool(), vm_id, VmStatus::Paused).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/resume",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM resumed successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn resume(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    // Call qarax-node to resume the VM
    let node_client = NodeClient::from_address(env.qarax_node_address());
    node_client.resume_vm(vm_id).await.map_err(|e| {
        tracing::error!("Failed to resume VM on qarax-node: {}", e);
        crate::errors::Error::InternalServerError
    })?;

    // Update status in database
    vms::update_status(env.pool(), vm_id, VmStatus::Running).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    delete,
    path = "/vms/{vm_id}",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 204, description = "VM deleted successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    // Call qarax-node to delete the VM
    let node_client = NodeClient::from_address(env.qarax_node_address());
    node_client.delete_vm(vm_id).await.map_err(|e| {
        tracing::error!("Failed to delete VM on qarax-node: {}", e);
        crate::errors::Error::InternalServerError
    })?;

    // Delete from database
    vms::delete(env.pool(), vm_id).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::NO_CONTENT,
    })
}
