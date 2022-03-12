use super::*;

use crate::handlers::ServerError;

#[derive(Error, Debug)]
pub enum VolumeError {
    #[error("Invalid volume name")]
    InvalidName(#[from] ValidationError),
    #[error("Invalid size: {0}")]
    InvalidSize(String),
    #[error("Storage not found: {0}")]
    StorageNotFound(String),
    #[error("Unexpected failure: {0}")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<sqlx::Error> for VolumeError {
    fn from(e: sqlx::Error) -> Self {
        VolumeError::Other(Box::new(e))
    }
}

impl From<VolumeError> for ServerError {
    fn from(e: VolumeError) -> Self {
        match e {
            VolumeError::InvalidName(e) => ServerError::Validation(e.to_string()),
            VolumeError::InvalidSize(e) => ServerError::Validation(e.to_string()),
            VolumeError::StorageNotFound(e) => ServerError::Validation(e.to_string()),
            VolumeError::Other(e) => ServerError::Internal(e.to_string()),
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct NewVolume {
    pub name: ValidName,
    pub size: Option<i64>,
    pub url: Option<String>,
    pub volume_type: VolumeType,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Volume {
    pub id: Uuid,
    pub name: String,
    pub size: i64,
    pub storage_id: Uuid,
    pub volume_type: VolumeType,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[strum(serialize_all = "snake_case")]
pub enum VolumeType {
    #[strum(serialize = "drive")]
    Drive,
    #[strum(serialize = "kernel")]
    Kernel,
}

pub async fn add(
    pool: &PgPool,
    volume: &NewVolume,
    storage_id: Uuid,
    size: i64,
) -> Result<Uuid, VolumeError> {
    let rec = sqlx::query!(
        r#"
INSERT INTO volumes (name, size, volume_type, storage_id)
VALUES ( $1, $2, $3, $4)
RETURNING id
        "#,
        volume.name.0,
        size,
        volume.volume_type.to_string(),
        storage_id,
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn by_id(pool: &PgPool, volume_id: Uuid) -> Result<Volume, VolumeError> {
    let volume = sqlx::query_as!(
        Volume,
        r#"
SELECT id, name, size, storage_id, volume_type as "volume_type: _"
FROM volumes
WHERE id = $1
        "#,
        volume_id
    )
    .fetch_one(pool)
    .await?;

    Ok(volume)
}
