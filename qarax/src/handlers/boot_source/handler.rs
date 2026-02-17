use super::*;
use crate::{
    App,
    model::boot_sources::{self, BootSource, NewBootSource},
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/boot-sources",
    responses(
        (status = 200, description = "List all boot sources", body = Vec<BootSource>),
        (status = 500, description = "Internal server error")
    ),
    tag = "boot-sources"
)]
#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<BootSource>>> {
    let boot_sources = boot_sources::list(env.pool()).await?;
    Ok(ApiResponse {
        data: boot_sources,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/boot-sources/{boot_source_id}",
    params(
        ("boot_source_id" = uuid::Uuid, Path, description = "Boot source unique identifier")
    ),
    responses(
        (status = 200, description = "Boot source found", body = BootSource),
        (status = 404, description = "Boot source not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "boot-sources"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(boot_source_id): Path<Uuid>,
) -> Result<ApiResponse<BootSource>> {
    let boot_source = boot_sources::get(env.pool(), boot_source_id).await?;
    Ok(ApiResponse {
        data: boot_source,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/boot-sources",
    request_body = NewBootSource,
    responses(
        (status = 201, description = "Boot source created successfully", body = String),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "boot-sources"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(new_boot_source): Json<NewBootSource>,
) -> Result<(StatusCode, String)> {
    let id = boot_sources::create(env.pool(), new_boot_source).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    delete,
    path = "/boot-sources/{boot_source_id}",
    params(
        ("boot_source_id" = uuid::Uuid, Path, description = "Boot source unique identifier")
    ),
    responses(
        (status = 204, description = "Boot source deleted successfully"),
        (status = 404, description = "Boot source not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "boot-sources"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(boot_source_id): Path<Uuid>,
) -> Result<StatusCode> {
    boot_sources::delete(env.pool(), boot_source_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
