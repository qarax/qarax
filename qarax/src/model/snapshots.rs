use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::model::storage_objects::StorageObject;

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "snapshot_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SnapshotStatus {
    Creating,
    Ready,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, sqlx::FromRow)]
pub struct Snapshot {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub storage_object_id: Uuid,
    pub name: String,
    pub status: SnapshotStatus,
    pub created_at: DateTime<Utc>,
}

pub struct NewSnapshot {
    pub vm_id: Uuid,
    pub storage_object_id: Uuid,
    pub name: String,
}

pub async fn create(pool: &PgPool, new: &NewSnapshot) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
INSERT INTO vm_snapshots (id, vm_id, storage_object_id, name)
VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(id)
    .bind(new.vm_id)
    .bind(new.storage_object_id)
    .bind(&new.name)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn get(pool: &PgPool, snapshot_id: Uuid) -> Result<Snapshot, sqlx::Error> {
    sqlx::query_as!(
        Snapshot,
        r#"
SELECT id, vm_id, storage_object_id, name, status as "status: _", created_at
FROM vm_snapshots
WHERE id = $1
        "#,
        snapshot_id
    )
    .fetch_one(pool)
    .await
}

pub async fn get_with_storage_object(
    pool: &PgPool,
    snapshot_id: Uuid,
) -> Result<(Snapshot, StorageObject), sqlx::Error> {
    let snapshot = get(pool, snapshot_id).await?;
    let so = crate::model::storage_objects::get(pool, snapshot.storage_object_id).await?;
    Ok((snapshot, so))
}

pub async fn list_for_vm(
    pool: &PgPool,
    vm_id: Uuid,
    name_filter: Option<&str>,
) -> Result<Vec<Snapshot>, sqlx::Error> {
    sqlx::query_as!(
        Snapshot,
        r#"
SELECT id, vm_id, storage_object_id, name, status as "status: _", created_at
FROM vm_snapshots
WHERE vm_id = $1 AND ($2::text IS NULL OR name = $2)
ORDER BY created_at ASC
        "#,
        vm_id,
        name_filter
    )
    .fetch_all(pool)
    .await
}

pub async fn update_status(
    pool: &PgPool,
    snapshot_id: Uuid,
    status: SnapshotStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE vm_snapshots
SET status = $2
WHERE id = $1
        "#,
    )
    .bind(snapshot_id)
    .bind(&status)
    .execute(pool)
    .await?;

    Ok(())
}
