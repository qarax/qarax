use sqlx::types::Json;

use crate::handlers::ServerError;

use super::*;

#[derive(Error, Debug)]
pub enum DriveError {
    #[error("Inavlid name: {0}")]
    InvalidName(#[from] ValidationError),
    #[error("Invalid size: {0}")]
    InvalidSize(String),
    #[error("Storage not found: {0}")]
    StorageNotFound(String),
    #[error("Unexpected failure: {0}")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<sqlx::Error> for DriveError {
    fn from(e: sqlx::Error) -> Self {
        DriveError::Other(Box::new(e))
    }
}

impl From<DriveError> for ServerError {
    fn from(e: DriveError) -> Self {
        match e {
            DriveError::InvalidName(e) => ServerError::Validation(e.to_string()),
            DriveError::InvalidSize(e) => ServerError::Validation(e.to_string()),
            DriveError::StorageNotFound(e) => ServerError::Validation(e.to_string()),
            DriveError::Other(e) => ServerError::Internal(e.to_string()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewDrive {
    pub name: ValidName,
    pub url: Option<String>,
    pub config: DriveConfig,
    pub storage_id: Uuid,
    pub volume_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, Type)]
pub struct DriveConfig {
    #[serde(default = "CacheType::default")]
    pub cache_type: CacheType,
    pub read_only: Option<bool>,
    pub root_device: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Drive {
    pub id: Uuid,
    pub config: Json<DriveConfig>,
    pub status: Status,
    pub volume_id: Uuid,
}

#[derive(Serialize, Deserialize, Debug, Clone, EnumString, Type)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "lowercase")]
pub enum CacheType {
    Unsafe,
    Writeback,
}

// TODO one day, change to #[default] Unsafe
impl Default for CacheType {
    fn default() -> Self {
        CacheType::Unsafe
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[strum(serialize_all = "snake_case")]
pub enum Status {
    #[strum(serialize = "plugged")]
    Plugged,
    #[strum(serialize = "unplugged")]
    Unplugged,
}

pub async fn add(pool: &PgPool, drive: &NewDrive) -> Result<Uuid, DriveError> {
    let rec = sqlx::query!(
        r#"
INSERT INTO drives (status, config, volume_id)
VALUES ( $1, $2, $3)
RETURNING id
        "#,
        Status::Unplugged as Status,
        Json(&drive.config) as _,
        drive.volume_id.unwrap(),
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn list(pool: &PgPool) -> Result<Vec<Drive>, DriveError> {
    let drives = sqlx::query_as!(
        Drive,
        r#"
SELECT id, volume_id, status as "status: _", config as "config: Json<DriveConfig>" 
FROM drives
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(drives)
}

pub async fn by_id(pool: &PgPool, drive_id: Uuid) -> Result<Drive, DriveError> {
    let drive = sqlx::query_as!(
        Drive,
        r#"
SELECT id, volume_id, status as "status: _", config as "config: Json<DriveConfig>" 
FROM drives
WHERE id = $1
        "#,
        drive_id
    )
    .fetch_one(pool)
    .await?;

    Ok(drive)
}
