use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    App,
    model::{
        sandbox_pool_members,
        sandbox_pools::{self, ConfigureSandboxPoolRequest, SandboxPool},
        vm_templates,
    },
    sandbox_pool_manager,
};

use super::super::{ApiResponse, Result};

#[utoipa::path(
    get,
    path = "/sandbox-pools",
    responses(
        (status = 200, description = "List configured sandbox pools", body = Vec<SandboxPool>),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandbox-pools"
)]
#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<SandboxPool>>> {
    let pools = sandbox_pools::list(env.pool()).await?;
    Ok(ApiResponse {
        data: pools,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/vm-templates/{vm_template_id}/sandbox-pool",
    params(
        ("vm_template_id" = uuid::Uuid, Path, description = "VM template unique identifier")
    ),
    responses(
        (status = 200, description = "Sandbox pool configuration", body = SandboxPool),
        (status = 404, description = "Sandbox pool not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandbox-pools"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(vm_template_id): Path<Uuid>,
) -> Result<ApiResponse<SandboxPool>> {
    let pool = sandbox_pools::get_by_template(env.pool(), vm_template_id).await?;
    Ok(ApiResponse {
        data: pool,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    put,
    path = "/vm-templates/{vm_template_id}/sandbox-pool",
    params(
        ("vm_template_id" = uuid::Uuid, Path, description = "VM template unique identifier")
    ),
    request_body = ConfigureSandboxPoolRequest,
    responses(
        (status = 200, description = "Sandbox pool configured", body = SandboxPool),
        (status = 422, description = "Invalid pool configuration"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandbox-pools"
)]
#[instrument(skip(env, body))]
pub async fn put(
    Extension(env): Extension<App>,
    Path(vm_template_id): Path<Uuid>,
    Json(body): Json<ConfigureSandboxPoolRequest>,
) -> Result<ApiResponse<SandboxPool>> {
    if body.min_ready < 0 {
        return Err(crate::errors::Error::UnprocessableEntity(
            "min_ready must be greater than or equal to 0".into(),
        ));
    }

    let template = vm_templates::get(env.pool(), vm_template_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;
    if template.image_ref.is_some() {
        return Err(crate::errors::Error::UnprocessableEntity(
            "sandbox VM templates with OCI image_ref are not supported yet".into(),
        ));
    }

    let pool = sandbox_pools::upsert(env.pool(), vm_template_id, body.min_ready)
        .await
        .map_err(crate::errors::Error::Sqlx)?;
    Ok(ApiResponse {
        data: pool,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    delete,
    path = "/vm-templates/{vm_template_id}/sandbox-pool",
    params(
        ("vm_template_id" = uuid::Uuid, Path, description = "VM template unique identifier")
    ),
    responses(
        (status = 204, description = "Sandbox pool deleted"),
        (status = 404, description = "Sandbox pool not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandbox-pools"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(vm_template_id): Path<Uuid>,
) -> Result<StatusCode> {
    let pool = sandbox_pools::get_config(env.pool(), vm_template_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?
        .ok_or(crate::errors::Error::NotFound)?;

    let members = sandbox_pool_members::list_by_pool(env.pool(), pool.id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;
    for member in members {
        sandbox_pool_manager::destroy_member(env.pool(), member).await;
    }

    let mut tx = env.pool().begin().await?;
    sandbox_pools::delete_tx(&mut tx, vm_template_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;
    tx.commit().await.map_err(crate::errors::Error::Sqlx)?;

    Ok(StatusCode::NO_CONTENT)
}
