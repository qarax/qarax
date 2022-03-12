use crate::models::drives as drives_model;
use axum::{extract::Path, Extension, Json};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    env::Environment,
    models::{
        drives::{DriveConfig, DriveError, NewDrive},
        volumes::{NewVolume, VolumeType},
        ValidName,
    },
};

use super::{storage::handler::create_volume_contcrete, ApiResponse, ServerError};

#[tracing::instrument(skip(env))]
pub async fn add(
    Extension(env): Extension<Environment>,
    Json(drive_request): Json<NewDriveRequest>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    // Create new volume
    let mut new_drive: NewDrive = drive_request.try_into()?;
    let new_volume = NewVolume::try_from(new_drive.clone())?;
    let volume_id = create_volume_contcrete(&new_drive.storage_id, new_volume, env.clone()).await?;
    new_drive.volume_id = Some(volume_id);

    // Create new drive object
    let drive_id = drives_model::add(env.db(), &new_drive).await?;

    Ok(ApiResponse {
        data: drive_id,
        code: StatusCode::OK,
    })
}

#[tracing::instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<drives_model::Drive>>, ServerError> {
    let drives = drives_model::list(env.db()).await?;

    Ok(ApiResponse {
        data: drives,
        code: StatusCode::OK,
    })
}

#[tracing::instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<Environment>,
    Path(drive_id): Path<Uuid>,
) -> Result<ApiResponse<drives_model::Drive>, ServerError> {
    let drive = drives_model::by_id(env.db(), drive_id).await?;

    Ok(ApiResponse {
        data: drive,
        code: StatusCode::OK,
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewDriveRequest {
    pub name: String,
    pub storage_type: String,
    pub config: DriveConfig,
    pub storage_id: Uuid,
    pub url: Url,
}

impl TryFrom<NewDriveRequest> for NewDrive {
    type Error = DriveError;

    fn try_from(value: NewDriveRequest) -> Result<Self, Self::Error> {
        // TODO validate storage id
        let name = ValidName::new(value.name)?;

        Ok(NewDrive {
            name,
            url: Some(value.url.to_string()),
            config: value.config,
            storage_id: value.storage_id,
            volume_id: None,
        })
    }
}

impl TryFrom<NewDrive> for NewVolume {
    type Error = DriveError;

    fn try_from(value: NewDrive) -> Result<Self, Self::Error> {
        Ok(NewVolume {
            name: value.name,
            size: None,
            url: value.url,
            volume_type: VolumeType::Drive,
        })
    }
}
