use super::*;
use crate::{
    App,
    model::{
        sandbox_pools,
        vm_templates::{self, NewVmTemplate, VmTemplate},
    },
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/vm-templates",
    params(crate::handlers::NameQuery),
    responses(
        (status = 200, description = "List all VM templates", body = Vec<VmTemplate>),
        (status = 500, description = "Internal server error")
    ),
    tag = "vm-templates"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::NameQuery>,
) -> Result<ApiResponse<Vec<VmTemplate>>> {
    let vm_templates = vm_templates::list(env.pool(), query.name.as_deref()).await?;
    Ok(ApiResponse {
        data: vm_templates,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/vm-templates/{vm_template_id}",
    params(
        ("vm_template_id" = uuid::Uuid, Path, description = "VM template unique identifier")
    ),
    responses(
        (status = 200, description = "VM template found", body = VmTemplate),
        (status = 404, description = "VM template not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vm-templates"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(vm_template_id): Path<Uuid>,
) -> Result<ApiResponse<VmTemplate>> {
    let vm_template = vm_templates::get(env.pool(), vm_template_id).await?;
    Ok(ApiResponse {
        data: vm_template,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/vm-templates",
    request_body = NewVmTemplate,
    responses(
        (status = 201, description = "VM template created successfully", body = String),
        (status = 409, description = "VM template with name already exists"),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vm-templates"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(new_vm_template): Json<NewVmTemplate>,
) -> Result<(StatusCode, String)> {
    let id = vm_templates::create(env.pool(), new_vm_template).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    delete,
    path = "/vm-templates/{vm_template_id}",
    params(
        ("vm_template_id" = uuid::Uuid, Path, description = "VM template unique identifier")
    ),
    responses(
        (status = 204, description = "VM template deleted successfully"),
        (status = 404, description = "VM template not found"),
        (status = 409, description = "VM template still has a sandbox pool"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vm-templates"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(vm_template_id): Path<Uuid>,
) -> Result<StatusCode> {
    // sandbox_pools.vm_template_id has no ON DELETE action: deleting the pool
    // tears down its prewarmed VMs, so require that to be done explicitly.
    match sandbox_pools::get_by_template(env.pool(), vm_template_id).await {
        Ok(pool) => {
            return Err(crate::errors::Error::Conflict(format!(
                "VM template {vm_template_id} has a sandbox pool ({}); delete the pool first \
                 (DELETE /vm-templates/{vm_template_id}/sandbox-pool)",
                pool.id
            )));
        }
        Err(sqlx::Error::RowNotFound) => {}
        Err(e) => return Err(e.into()),
    }
    vm_templates::delete(env.pool(), vm_template_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
