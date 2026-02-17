use super::*;
use crate::{
    App,
    model::hosts::{self, Host, NewHost, UpdateHostRequest},
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/hosts",
    responses(
        (status = 200, description = "List all hosts", body = Vec<Host>),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<Host>>> {
    let hosts = hosts::list(env.pool()).await?;
    Ok(ApiResponse {
        data: hosts,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/hosts",
    request_body = NewHost,
    responses(
        (status = 201, description = "Host created successfully", body = String),
        (status = 422, description = "Invalid input"),
        (status = 409, description = "Host with name already exists"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn add(
    Extension(env): Extension<App>,
    Json(host): Json<NewHost>,
) -> Result<(StatusCode, String)> {
    host.validate_unique_name(env.pool(), &host.name).await?;
    let id = hosts::add(env.pool(), &host).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    patch,
    path = "/hosts/{host_id}",
    params(
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    request_body = UpdateHostRequest,
    responses(
        (status = 200, description = "Host updated successfully"),
        (status = 404, description = "Host not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn update(
    Extension(env): Extension<App>,
    Path(host_id): Path<Uuid>,
    Json(body): Json<UpdateHostRequest>,
) -> Result<ApiResponse<()>> {
    hosts::update_status(env.pool(), host_id, body.status).await?;
    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}
