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
    pub name: String,
    pub status: SnapshotStatus,
    pub snapshot_url: String,
    pub created_at: DateTime<Utc>,
}

pub struct NewSnapshot {
    pub vm_id: Uuid,
    pub name: String,
    pub snapshot_url: String,
}

pub async fn create(pool: &PgPool, new: &NewSnapshot) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
INSERT INTO vm_snapshots (id, vm_id, name, snapshot_url)
VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(id)
    .bind(new.vm_id)
    .bind(&new.name)
    .bind(&new.snapshot_url)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn get(pool: &PgPool, snapshot_id: Uuid) -> Result<Snapshot, sqlx::Error> {
    let snapshot = sqlx::query_as::<_, Snapshot>(
        r#"
SELECT id, vm_id, name, status, snapshot_url, created_at
FROM vm_snapshots
WHERE id = $1
        "#,
    )
    .bind(snapshot_id)
    .fetch_one(pool)
    .await?;

    Ok(snapshot)
}

pub async fn list_for_vm(pool: &PgPool, vm_id: Uuid) -> Result<Vec<Snapshot>, sqlx::Error> {
    let snapshots = sqlx::query_as::<_, Snapshot>(
        r#"
SELECT id, vm_id, name, status, snapshot_url, created_at
FROM vm_snapshots
WHERE vm_id = $1
ORDER BY created_at ASC
        "#,
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;

    Ok(snapshots)
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
