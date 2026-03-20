use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use super::storage_objects::StorageObjectType;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Transfer {
    pub id: Uuid,
    pub name: String,
    pub transfer_type: TransferType,
    pub status: TransferStatus,
    pub source: String,
    pub storage_pool_id: Uuid,
    pub object_type: StorageObjectType,
    pub storage_object_id: Option<Uuid>,
    pub total_bytes: Option<i64>,
    pub transferred_bytes: i64,
    pub error_message: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
}

#[derive(sqlx::FromRow)]
pub struct TransferRow {
    pub id: Uuid,
    pub name: String,
    pub transfer_type: TransferType,
    pub status: TransferStatus,
    pub source: String,
    pub storage_pool_id: Uuid,
    pub object_type: StorageObjectType,
    pub storage_object_id: Option<Uuid>,
    pub total_bytes: Option<i64>,
    pub transferred_bytes: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
}

impl From<TransferRow> for Transfer {
    fn from(row: TransferRow) -> Self {
        Transfer {
            id: row.id,
            name: row.name,
            transfer_type: row.transfer_type,
            status: row.status,
            source: row.source,
            storage_pool_id: row.storage_pool_id,
            object_type: row.object_type,
            storage_object_id: row.storage_object_id,
            total_bytes: row.total_bytes,
            transferred_bytes: row.transferred_bytes.unwrap_or(0),
            error_message: row.error_message,
            created_at: row.created_at,
            updated_at: row.updated_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
        }
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "transfer_type")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TransferType {
    Download,
    LocalCopy,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "transfer_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TransferStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewTransfer {
    pub name: String,
    pub source: String,
    pub object_type: StorageObjectType,
}

pub async fn create(
    pool: &PgPool,
    storage_pool_id: Uuid,
    new_transfer: &NewTransfer,
    transfer_type: TransferType,
) -> Result<Transfer, sqlx::Error> {
    let id = Uuid::new_v4();

    let row: TransferRow = sqlx::query_as!(
        TransferRow,
        r#"
INSERT INTO transfers (id, name, transfer_type, source, storage_pool_id, object_type)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING id,
          name,
          transfer_type as "transfer_type: _",
          status as "status: _",
          source,
          storage_pool_id,
          object_type as "object_type: _",
          storage_object_id as "storage_object_id?",
          total_bytes as "total_bytes?",
          transferred_bytes as "transferred_bytes?",
          error_message as "error_message?",
          created_at as "created_at?",
          updated_at as "updated_at?",
          started_at as "started_at?",
          completed_at as "completed_at?"
        "#,
        id,
        new_transfer.name,
        transfer_type as TransferType,
        new_transfer.source,
        storage_pool_id,
        new_transfer.object_type.clone() as StorageObjectType,
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn get(pool: &PgPool, transfer_id: Uuid) -> Result<Transfer, sqlx::Error> {
    let row: TransferRow = sqlx::query_as!(
        TransferRow,
        r#"
SELECT id,
       name,
       transfer_type as "transfer_type: _",
       status as "status: _",
       source,
       storage_pool_id,
       object_type as "object_type: _",
       storage_object_id as "storage_object_id?",
       total_bytes as "total_bytes?",
       transferred_bytes as "transferred_bytes?",
       error_message as "error_message?",
       created_at as "created_at?",
       updated_at as "updated_at?",
       started_at as "started_at?",
       completed_at as "completed_at?"
FROM transfers
WHERE id = $1
        "#,
        transfer_id
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn list_by_pool(
    pool: &PgPool,
    pool_id: Uuid,
    name_filter: Option<&str>,
) -> Result<Vec<Transfer>, sqlx::Error> {
    let rows: Vec<TransferRow> = sqlx::query_as!(
        TransferRow,
        r#"
SELECT id,
       name,
       transfer_type as "transfer_type: _",
       status as "status: _",
       source,
       storage_pool_id,
       object_type as "object_type: _",
       storage_object_id as "storage_object_id?",
       total_bytes as "total_bytes?",
       transferred_bytes as "transferred_bytes?",
       error_message as "error_message?",
       created_at as "created_at?",
       updated_at as "updated_at?",
       started_at as "started_at?",
       completed_at as "completed_at?"
FROM transfers
WHERE storage_pool_id = $1 AND ($2::text IS NULL OR name = $2)
ORDER BY created_at DESC
        "#,
        pool_id,
        name_filter
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn mark_running(pool: &PgPool, transfer_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
UPDATE transfers
SET status = 'RUNNING',
    started_at = CURRENT_TIMESTAMP,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $1
        "#,
        transfer_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_completed(
    pool: &PgPool,
    transfer_id: Uuid,
    storage_object_id: Uuid,
    bytes_written: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
UPDATE transfers
SET status = 'COMPLETED',
    storage_object_id = $2,
    transferred_bytes = $3,
    completed_at = CURRENT_TIMESTAMP,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $1
        "#,
        transfer_id,
        storage_object_id,
        bytes_written,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_failed(
    pool: &PgPool,
    transfer_id: Uuid,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
UPDATE transfers
SET status = 'FAILED',
    error_message = $2,
    completed_at = CURRENT_TIMESTAMP,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $1
        "#,
        transfer_id,
        error_message,
    )
    .execute(pool)
    .await?;

    Ok(())
}
