use crate::models::kernels::{self as kernel_model, KernelError, NewKernel};
use axum::{extract::Path, Extension, Json};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    env::Environment,
    models::{
        volumes::{NewVolume, VolumeType},
        ValidName,
    },
};

use super::{storage::handler::create_volume_concrete, ApiResponse, ServerError};

#[tracing::instrument(skip(env))]
pub async fn add(
    Extension(env): Extension<Environment>,
    Json(kernel_request): Json<NewKernelRequest>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    // Create new volume
    let mut new_kernel: NewKernel = kernel_request.try_into()?;
    let new_volume = NewVolume::try_from(new_kernel.clone())?;
    let volume_id =
        create_volume_concrete(&new_kernel.storage_id, new_volume, env.clone()).await?;
    new_kernel.volume_id = Some(volume_id);

    let kernel_id = kernel_model::add(env.db(), &new_kernel).await?;

    Ok(ApiResponse {
        data: kernel_id,
        code: StatusCode::OK,
    })
}

#[tracing::instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<kernel_model::Kernel>>, ServerError> {
    let kernels = kernel_model::list(env.db()).await?;

    Ok(ApiResponse {
        data: kernels,
        code: StatusCode::OK,
    })
}

#[tracing::instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<Environment>,
    Path(kernel_id): Path<Uuid>,
) -> Result<ApiResponse<kernel_model::Kernel>, ServerError> {
    let kernel = kernel_model::by_id(env.db(), kernel_id).await?;

    Ok(ApiResponse {
        data: kernel,
        code: StatusCode::OK,
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewKernelRequest {
    pub name: String,
    pub storage_id: Uuid,
    pub url: Url,
}

impl TryFrom<NewKernelRequest> for NewKernel {
    type Error = KernelError;

    fn try_from(value: NewKernelRequest) -> Result<Self, Self::Error> {
        // TODO validate storage id
        let name = ValidName::new(value.name)?;

        Ok(NewKernel {
            name,
            url: Some(value.url.to_string()),
            storage_id: value.storage_id,
            volume_id: None,
        })
    }
}

impl TryFrom<NewKernel> for NewVolume {
    type Error = KernelError;

    fn try_from(value: NewKernel) -> Result<Self, Self::Error> {
        Ok(NewVolume {
            name: value.name,
            size: None,
            url: value.url,
            volume_type: VolumeType::Kernel,
        })
    }
}
