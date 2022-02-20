use std::convert::TryFrom;

use super::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Drive {
    pub id: Uuid,
    pub name: String,
    pub status: Status,
    pub cache_type: CacheType,
    pub readonly: bool,
    pub rootfs: bool,
    pub storage_id: Uuid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewDrive {
    pub name: String,
    pub readonly: bool,
    pub rootfs: bool,
    pub cache_type: CacheType,
    pub storage_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Type, EnumString, Display)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[strum(serialize_all = "snake_case")]
pub enum CacheType {
    None,
    Unsafe,
    Writeback,
}

impl TryFrom<i32> for CacheType {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(CacheType::None),
            1 => Ok(CacheType::Unsafe),
            2 => Ok(CacheType::Writeback),
            _ => panic!("should never happen!"),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[strum(serialize_all = "snake_case")]
pub enum Status {
    #[strum(serialize = "up")]
    Up,
    #[strum(serialize = "down")]
    Down,
}

#[derive(Error, Debug)]
pub enum DriveError {
    #[error("Failed to list drives: {0}")]
    List(#[from] sqlx::Error),

    #[error("Failed to add drive: {0}, error: {1}")]
    Add(String, sqlx::Error),

    #[error("Can't find drive: {0}, error: {1}")]
    Find(Uuid, sqlx::Error),
}

pub async fn list(pool: &PgPool) -> Result<Vec<Drive>, DriveError> {
    let drives = sqlx::query_as!(
        Drive,
        r#"
SELECT id, name, status as "status: _", readonly, rootfs, storage_id, cache_type as "cache_type: _"
FROM drives
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(DriveError::List)?;

    Ok(drives)
}

pub async fn by_id(pool: &PgPool, drive_id: &Uuid) -> Result<Drive, DriveError> {
    let drive = sqlx::query_as!(
        Drive,
        r#"
SELECT id, name, status as "status: _", readonly, rootfs, storage_id, cache_type as "cache_type: _"
FROM drives
WHERE id = $1
        "#,
        drive_id
    )
    .fetch_one(pool)
    .await
    .map_err(|e| DriveError::Find(*drive_id, e))?;

    Ok(drive)
}

pub async fn add(pool: &PgPool, drive: &NewDrive) -> Result<Uuid, DriveError> {
    let rec = sqlx::query!(
        r#"
INSERT INTO drives (name, status, readonly, rootfs, storage_id, cache_type)
VALUES ( $1, $2, $3, $4, $5, $6)
RETURNING id
        "#,
        drive.name,
        Status::Down as Status,
        drive.readonly,
        drive.rootfs,
        drive.storage_id,
        drive.cache_type as CacheType
    )
    .fetch_one(pool)
    .await
    .map_err(|e| DriveError::Add(drive.name.to_owned(), e))?;

    Ok(rec.id)
}
