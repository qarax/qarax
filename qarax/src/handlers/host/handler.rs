use super::*;
use crate::{
    App,
    model::hosts::{self, Host, NewHost},
};
use axum::{Extension, Json};
use http::StatusCode;
use tracing::instrument;

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
