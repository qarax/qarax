use super::*;

use axum::extract::{Json, Path};

use crate::models::drives as drive_model;
use crate::models::drives::{Drive, NewDrive};

pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<Drive>>, ServerError> {
    let drives = drive_model::list(env.db())
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(ApiResponse {
        data: drives,
        code: StatusCode::OK,
    })
}

pub async fn add(
    Extension(env): Extension<Environment>,
    Json(drive): Json<NewDrive>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let drive_id = drive_model::add(env.db(), &drive)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(ApiResponse {
        data: drive_id,
        code: StatusCode::CREATED,
    })
}

pub async fn get(
    Extension(env): Extension<Environment>,
    Path(drive_id): Path<Uuid>,
) -> Result<ApiResponse<Drive>, ServerError> {
    let drive = drive_model::by_id(env.db(), &drive_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(ApiResponse {
        data: drive,
        code: StatusCode::CREATED,
    })
}
