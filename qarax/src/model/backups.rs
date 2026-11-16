use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::pagination::PageRequest;

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
    pub vm_id: Option<Uuid>,
    pub snapshot_id: Option<Uuid>,
    pub storage_object_id: Uuid,
}

/// Record a backup whose data is already stored. Backups are only recorded
/// once complete, so this version writes every one as `READY`: `CREATING`,
/// `FAILED` and `error_message` stay in the schema and API for compatibility.
pub async fn create(pool: &PgPool, new_backup: &NewBackup) -> Result<Backup, sqlx::Error> {
    let row = sqlx::query_as!(
        BackupRow,
        r#"
INSERT INTO backups (id, name, backup_type, status, vm_id, snapshot_id, storage_object_id)
VALUES ($1, $2, $3, 'READY', $4, $5, $6)
RETURNING id,
          name,
          backup_type AS "backup_type: _",
          status AS "status: _",
          vm_id,
          snapshot_id,
          storage_object_id,
          error_message,
          created_at,
          updated_at
        "#,
        Uuid::new_v4(),
        &new_backup.name,
        &new_backup.backup_type as &BackupType,
        new_backup.vm_id,
        new_backup.snapshot_id,
        new_backup.storage_object_id
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn get(pool: &PgPool, backup_id: Uuid) -> Result<Backup, sqlx::Error> {
    let row = sqlx::query_as!(
        BackupRow,
        r#"
SELECT id,
       name,
       backup_type AS "backup_type: _",
       status AS "status: _",
       vm_id,
       snapshot_id,
       storage_object_id,
       error_message,
       created_at,
       updated_at
FROM backups
WHERE id = $1
        "#,
        backup_id
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

/// A database backup's records: the backup and its dump's storage object.
#[derive(Debug, Clone)]
pub struct DatabaseBackupRecord {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub storage_object_id: Uuid,
    pub object_name: String,
    pub storage_pool_id: Uuid,
    pub size_bytes: i64,
    pub object_config: serde_json::Value,
}

pub async fn list_database_backup_records(
    pool: &PgPool,
) -> Result<Vec<DatabaseBackupRecord>, sqlx::Error> {
    sqlx::query_as!(
        DatabaseBackupRecord,
        r#"
SELECT b.id,
       b.name,
       b.created_at,
       b.storage_object_id,
       o.name AS "object_name!",
       o.storage_pool_id AS "storage_pool_id!",
       o.size_bytes AS "size_bytes!",
       o.config AS "object_config!"
FROM backups b
JOIN storage_objects o ON o.id = b.storage_object_id
WHERE b.backup_type = 'DATABASE'
        "#
    )
    .fetch_all(pool)
    .await
}

/// Makes the database backups' records `records` again, as listed before a
/// database restore: drops the others and re-creates missing ones with their
/// IDs. Returns those that couldn't be re-created (e.g. their pool is gone),
/// with why.
pub async fn reset_database_backup_records(
    pool: &PgPool,
    records: &[DatabaseBackupRecord],
) -> Result<Vec<(DatabaseBackupRecord, sqlx::Error)>, sqlx::Error> {
    let ids: Vec<Uuid> = records.iter().map(|r| r.id).collect();
    // Deleting a backup's storage object deletes the backup.
    sqlx::query!(
        r#"
DELETE FROM storage_objects
WHERE id IN (SELECT storage_object_id FROM backups
             WHERE backup_type = 'DATABASE' AND NOT (id = ANY($1)))
        "#,
        &ids
    )
    .execute(pool)
    .await?;

    let mut failed = Vec::new();
    for record in records {
        if let Err(e) = recreate_database_backup_record(pool, record).await {
            failed.push((record.clone(), e));
        }
    }
    Ok(failed)
}

/// Inserts `record`'s storage object and backup unless they exist.
async fn recreate_database_backup_record(
    pool: &PgPool,
    record: &DatabaseBackupRecord,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"
INSERT INTO storage_objects (id, name, storage_pool_id, object_type, size_bytes, config)
VALUES ($1, $2, $3, 'DATABASE_BACKUP', $4, $5)
ON CONFLICT (id) DO NOTHING
        "#,
        record.storage_object_id,
        &record.object_name,
        record.storage_pool_id,
        record.size_bytes,
        &record.object_config
    )
    .execute(tx.as_mut())
    .await?;
    sqlx::query!(
        r#"
INSERT INTO backups (id, name, backup_type, status, storage_object_id, created_at)
VALUES ($1, $2, 'DATABASE', 'READY', $3, $4)
ON CONFLICT (id) DO NOTHING
        "#,
        record.id,
        &record.name,
        record.storage_object_id,
        record.created_at
    )
    .execute(tx.as_mut())
    .await?;
    tx.commit().await
}

/// Newest first.
pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
    type_filter: Option<BackupType>,
    page: &PageRequest,
) -> Result<Vec<Backup>, sqlx::Error> {
    let rows = sqlx::query_as!(
        BackupRow,
        r#"
SELECT id,
       name,
       backup_type AS "backup_type: _",
       status AS "status: _",
       vm_id,
       snapshot_id,
       storage_object_id,
       error_message,
       created_at,
       updated_at
FROM backups
WHERE ($1::text IS NULL OR name = $1)
  AND ($2::backup_type IS NULL OR backup_type = $2)
  AND ($3::text IS NULL OR (created_at, id) < ($3::timestamptz, $4::uuid))
ORDER BY created_at DESC, id DESC
LIMIT $5
        "#,
        name_filter,
        type_filter as Option<BackupType>,
        page.after_key(),
        page.after_id(),
        page.fetch_limit()
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::{
        storage_objects::{self, NewStorageObject, StorageObjectType},
        storage_pools::{self, NewStoragePool, StoragePoolType},
    };
    use crate::test_support::TestDatabase;

    async fn database_backup(pool: &PgPool, pool_id: Uuid, name: &str) -> Backup {
        let object = storage_objects::create_returning(
            pool,
            NewStorageObject {
                name: name.to_string(),
                storage_pool_id: Some(pool_id),
                object_type: StorageObjectType::DatabaseBackup,
                size_bytes: 10,
                config: json!({ "path": format!("/backups/{name}.dump") }),
                parent_id: None,
            },
        )
        .await
        .unwrap();
        create(
            pool,
            &NewBackup {
                name: name.to_string(),
                backup_type: BackupType::Database,
                vm_id: None,
                snapshot_id: None,
                storage_object_id: object.id,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn resetting_database_backup_records_undoes_a_restore_of_them() {
        let db = TestDatabase::new().await;
        let pool_id = storage_pools::create(
            &db.pool,
            NewStoragePool {
                name: "backups".into(),
                pool_type: StoragePoolType::Local,
                config: json!({ "path": "/backups" }),
                capacity_bytes: None,
            },
        )
        .await
        .unwrap();
        let older = database_backup(&db.pool, pool_id, "older").await;
        let newer = database_backup(&db.pool, pool_id, "newer").await;
        let records = list_database_backup_records(&db.pool).await.unwrap();

        // Restoring a dump taken after `older` and before `newer` drops
        // `newer`, and brings back `deleted`, which was deleted since.
        storage_objects::delete(&db.pool, newer.storage_object_id)
            .await
            .unwrap();
        database_backup(&db.pool, pool_id, "deleted").await;

        let failed = reset_database_backup_records(&db.pool, &records)
            .await
            .unwrap();
        assert!(failed.is_empty(), "{failed:?}");

        let mut after = list(
            &db.pool,
            None,
            Some(BackupType::Database),
            &PageRequest::all(),
        )
        .await
        .unwrap();
        after.sort_by_key(|b| b.created_at);
        let ids: Vec<_> = after.iter().map(|b| (b.id, b.storage_object_id)).collect();
        assert_eq!(
            ids,
            vec![
                (older.id, older.storage_object_id),
                (newer.id, newer.storage_object_id)
            ]
        );
        assert_eq!(after[1].created_at, newer.created_at);
        let object = storage_objects::get(&db.pool, newer.storage_object_id)
            .await
            .unwrap();
        assert_eq!(object.config, json!({ "path": "/backups/newer.dump" }));
        assert_eq!(
            storage_pools::get(&db.pool, pool_id)
                .await
                .unwrap()
                .allocated_bytes,
            Some(20)
        );
    }
}
