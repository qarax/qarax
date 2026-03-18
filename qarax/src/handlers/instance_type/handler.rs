use super::*;
use crate::{
    App,
    model::instance_types::{self, InstanceType, NewInstanceType},
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/instance-types",
    responses(
        (status = 200, description = "List all instance types", body = Vec<InstanceType>),
        (status = 500, description = "Internal server error")
    ),
    tag = "instance-types"
)]
#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<InstanceType>>> {
    let instance_types = instance_types::list(env.pool()).await?;
    Ok(ApiResponse {
        data: instance_types,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/instance-types/{instance_type_id}",
    params(
        ("instance_type_id" = uuid::Uuid, Path, description = "Instance type unique identifier")
    ),
    responses(
        (status = 200, description = "Instance type found", body = InstanceType),
        (status = 404, description = "Instance type not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "instance-types"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(instance_type_id): Path<Uuid>,
) -> Result<ApiResponse<InstanceType>> {
    let instance_type = instance_types::get(env.pool(), instance_type_id).await?;
    Ok(ApiResponse {
        data: instance_type,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/instance-types",
    request_body = NewInstanceType,
    responses(
        (status = 201, description = "Instance type created successfully", body = String),
        (status = 409, description = "Instance type with name already exists"),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "instance-types"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(new_instance_type): Json<NewInstanceType>,
) -> Result<(StatusCode, String)> {
    let id = instance_types::create(env.pool(), new_instance_type).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    delete,
    path = "/instance-types/{instance_type_id}",
    params(
        ("instance_type_id" = uuid::Uuid, Path, description = "Instance type unique identifier")
    ),
    responses(
        (status = 204, description = "Instance type deleted successfully"),
        (status = 404, description = "Instance type not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "instance-types"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(instance_type_id): Path<Uuid>,
) -> Result<StatusCode> {
    instance_types::delete(env.pool(), instance_type_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
