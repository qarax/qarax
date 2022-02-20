use super::*;

use axum::extract::{Json, Path};

use crate::models::kernels as kernel_model;
use crate::models::kernels::{Kernel, NewKernel};

pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<Kernel>>, ServerError> {
    let kernels = kernel_model::list(env.db())
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(ApiResponse {
        data: kernels,
        code: StatusCode::OK,
    })
}

pub async fn add(
    Extension(env): Extension<Environment>,
    Json(kernel): Json<NewKernel>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let kernel_id = kernel_model::add(env.db(), &kernel)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(ApiResponse {
        data: kernel_id,
        code: StatusCode::CREATED,
    })
}

pub async fn get(
    Extension(env): Extension<Environment>,
    Path(kernel_id): Path<Uuid>,
) -> Result<ApiResponse<Kernel>, ServerError> {
    let kernel = kernel_model::by_id(env.db(), &kernel_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(ApiResponse {
        data: kernel,
        code: StatusCode::CREATED,
    })
}
