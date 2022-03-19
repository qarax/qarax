use super::*;

use crate::handlers::rpc::node::{Drive as VmDrive, VmConfig};
use crate::models::vms as vm_model;
use crate::models::vms::{NewVm, Vm};
use axum::extract::{Json, Path};

pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<Vm>>, ServerError> {
    let vms = vm_model::list(env.db()).await?;

    Ok(ApiResponse {
        data: vms,
        code: StatusCode::OK,
    })
}

pub async fn add(
    Extension(env): Extension<Environment>,
    Json(vm): Json<NewVm>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let vm_id = vm_model::add(env.db(), &vm).await?;

    Ok(ApiResponse {
        data: vm_id,
        code: StatusCode::CREATED,
    })
}

pub async fn get(
    Extension(env): Extension<Environment>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<Vm>, ServerError> {
    let vm = vm_model::by_id(env.db(), &vm_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

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
    let clients = &*env.vmm_clients().read().await;

    let client = clients.get(&host.id).unwrap();
    let vm = vm_model::by_id(env.db(), &vm_id).await?;

    todo!()
}
