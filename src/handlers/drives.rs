use super::*;

use axum::extract::{Json, Path};

use models::drives as drive_model;
use models::drives::{Drive, NewDrive};

pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<Drive>>, ServerError> {
    let drives = drive_model::list(env.db()).await.map_err(|e| {
        tracing::error!("Failed to list drives, error: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        data: drives,
        code: StatusCode::OK,
    })
}

pub async fn add(
    Extension(env): Extension<Environment>,
    Json(drive): Json<NewDrive>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let drive_id = drive_model::add(env.db(), &drive).await.map_err(|e| {
        tracing::error!("Can't add drive: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        data: drive_id,
        code: StatusCode::CREATED,
    })
}

pub async fn get(
    Extension(env): Extension<Environment>,
    Path(drive_id): Path<Uuid>,
) -> Result<ApiResponse<Drive>, ServerError> {
    let drive = drive_model::by_id(env.db(), &drive_id).await.map_err(|e| {
        tracing::error!("Can't find drive, error: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        data: drive,
        code: StatusCode::CREATED,
    })
}
