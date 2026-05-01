use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "backup_type")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum BackupType {
    Vm,
    Database,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "backup_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum BackupStatus {
    Creating,
    Ready,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Backup {
    pub id: Uuid,
    pub name: String,
    pub backup_type: BackupType,
    pub status: BackupStatus,
    pub vm_id: Option<Uuid>,
    pub snapshot_id: Option<Uuid>,
    pub storage_object_id: Uuid,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct BackupRow {
    pub id: Uuid,
    pub name: String,
    pub backup_type: BackupType,
    pub status: BackupStatus,
    pub vm_id: Option<Uuid>,
    pub snapshot_id: Option<Uuid>,
    pub storage_object_id: Uuid,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<BackupRow> for Backup {
    fn from(row: BackupRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            backup_type: row.backup_type,
            status: row.status,
            vm_id: row.vm_id,
            snapshot_id: row.snapshot_id,
            storage_object_id: row.storage_object_id,
            error_message: row.error_message,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

pub struct NewBackup {
    pub name: String,
    pub backup_type: BackupType,
    pub status: BackupStatus,
    pub vm_id: Option<Uuid>,
    pub snapshot_id: Option<Uuid>,
    pub storage_object_id: Uuid,
}

pub async fn create(pool: &PgPool, new_backup: &NewBackup) -> Result<Backup, sqlx::Error> {
    let row = sqlx::query_as::<_, BackupRow>(
        r#"
INSERT INTO backups (id, name, backup_type, status, vm_id, snapshot_id, storage_object_id)
VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING id,
          name,
          backup_type,
          status,
          vm_id,
          snapshot_id,
          storage_object_id,
          error_message,
          created_at,
          updated_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&new_backup.name)
    .bind(&new_backup.backup_type)
    .bind(&new_backup.status)
    .bind(new_backup.vm_id)
    .bind(new_backup.snapshot_id)
    .bind(new_backup.storage_object_id)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn get(pool: &PgPool, backup_id: Uuid) -> Result<Backup, sqlx::Error> {
    let row = sqlx::query_as::<_, BackupRow>(
        r#"
SELECT id,
       name,
       backup_type,
       status,
       vm_id,
       snapshot_id,
       storage_object_id,
       error_message,
       created_at,
       updated_at
FROM backups
WHERE id = $1
        "#,
    )
    .bind(backup_id)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
    type_filter: Option<BackupType>,
) -> Result<Vec<Backup>, sqlx::Error> {
    let rows = sqlx::query_as::<_, BackupRow>(
        r#"
SELECT id,
       name,
       backup_type,
       status,
       vm_id,
       snapshot_id,
       storage_object_id,
       error_message,
       created_at,
       updated_at
FROM backups
WHERE ($1::text IS NULL OR name = $1)
  AND ($2::backup_type IS NULL OR backup_type = $2)
ORDER BY created_at DESC
        "#,
    )
    .bind(name_filter)
    .bind(type_filter)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn update_status(
    pool: &PgPool,
    backup_id: Uuid,
    status: BackupStatus,
    error_message: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE backups
SET status = $2,
    error_message = $3
WHERE id = $1
        "#,
    )
    .bind(backup_id)
    .bind(status)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}
