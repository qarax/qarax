use super::*;
use crate::{
    App,
    model::storage_pools::{self, NewStoragePool, StoragePool},
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/storage-pools",
    responses(
        (status = 200, description = "List all storage pools", body = Vec<StoragePool>),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<StoragePool>>> {
    let pools = storage_pools::list(env.pool()).await?;
    Ok(ApiResponse {
        data: pools,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/storage-pools/{pool_id}",
    params(
        ("pool_id" = uuid::Uuid, Path, description = "Storage pool unique identifier")
    ),
    responses(
        (status = 200, description = "Storage pool found", body = StoragePool),
        (status = 404, description = "Storage pool not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(pool_id): Path<Uuid>,
) -> Result<ApiResponse<StoragePool>> {
    let pool = storage_pools::get(env.pool(), pool_id).await?;
    Ok(ApiResponse {
        data: pool,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/storage-pools",
    request_body = NewStoragePool,
    responses(
        (status = 201, description = "Storage pool created successfully", body = String),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(new_pool): Json<NewStoragePool>,
) -> Result<(StatusCode, String)> {
    let id = storage_pools::create(env.pool(), new_pool).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    delete,
    path = "/storage-pools/{pool_id}",
    params(
        ("pool_id" = uuid::Uuid, Path, description = "Storage pool unique identifier")
    ),
    responses(
        (status = 204, description = "Storage pool deleted successfully"),
        (status = 404, description = "Storage pool not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(pool_id): Path<Uuid>,
) -> Result<StatusCode> {
    storage_pools::delete(env.pool(), pool_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
