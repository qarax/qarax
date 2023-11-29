use super::*;
use crate::{
    model::hosts::{self, Host, NewHost},
    App,
};
use axum::{Extension, Json};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<Host>>> {
    let hosts = hosts::list(env.pool()).await?;
    Ok(ApiResponse {
        data: hosts,
        code: StatusCode::OK,
    })
}

#[instrument(skip(env))]
pub async fn add(
    Extension(env): Extension<App>,
    Json(host): Json<NewHost>,
) -> Result<ApiResponse<Uuid>> {
    host.validate_unique_name(env.pool(), &host.name).await?;
    let id = hosts::add(env.pool(), &host).await?;
    Ok(ApiResponse {
        data: id,
        code: StatusCode::CREATED,
    })
}
