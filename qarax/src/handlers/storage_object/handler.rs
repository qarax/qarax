use super::*;
use crate::{
    App,
    model::storage_objects::{self, NewStorageObject, StorageObject},
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/storage-objects",
    responses(
        (status = 200, description = "List all storage objects", body = Vec<StorageObject>),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-objects"
)]
#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<StorageObject>>> {
    let objects = storage_objects::list(env.pool()).await?;
    Ok(ApiResponse {
        data: objects,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/storage-objects/{object_id}",
    params(
        ("object_id" = uuid::Uuid, Path, description = "Storage object unique identifier")
    ),
    responses(
        (status = 200, description = "Storage object found", body = StorageObject),
        (status = 404, description = "Storage object not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-objects"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(object_id): Path<Uuid>,
) -> Result<ApiResponse<StorageObject>> {
    let object = storage_objects::get(env.pool(), object_id).await?;
    Ok(ApiResponse {
        data: object,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/storage-objects",
    request_body = NewStorageObject,
    responses(
        (status = 201, description = "Storage object created successfully", body = String),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-objects"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(new_object): Json<NewStorageObject>,
) -> Result<(StatusCode, String)> {
    let id = storage_objects::create(env.pool(), new_object).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    delete,
    path = "/storage-objects/{object_id}",
    params(
        ("object_id" = uuid::Uuid, Path, description = "Storage object unique identifier")
    ),
    responses(
        (status = 204, description = "Storage object deleted successfully"),
        (status = 404, description = "Storage object not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-objects"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(object_id): Path<Uuid>,
) -> Result<StatusCode> {
    storage_objects::delete(env.pool(), object_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
