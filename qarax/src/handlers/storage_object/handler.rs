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
    params(crate::handlers::StorageObjectListQuery),
    responses(
        (status = 200, description = "List all storage objects", body = Vec<StorageObject>),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-objects"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::StorageObjectListQuery>,
) -> Result<ApiResponse<Vec<StorageObject>>> {
    let objects = storage_objects::list(
        env.pool(),
        query.name.as_deref(),
        query.pool_id,
        query.object_type,
    )
    .await?;
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
        (status = 202, description = "Deletion accepted; node file cleanup will be retried"),
        (status = 404, description = "Storage object not found"),
        (status = 409, description = "Storage object is still in use"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-objects"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(object_id): Path<Uuid>,
) -> Result<StatusCode> {
    let creating: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM jobs WHERE resource_id = $1 AND status IN ('PENDING', 'RUNNING'))",
    ).bind(object_id).fetch_one(env.pool()).await?;
    if creating {
        return Err(crate::errors::Error::Conflict(
            "Storage object is still being created".into(),
        ));
    }
    storage_objects::delete(env.pool(), object_id).await?;
    Ok(
        if crate::storage_cleanup::process(env.pool(), object_id).await? {
            StatusCode::NO_CONTENT
        } else {
            StatusCode::ACCEPTED
        },
    )
}
