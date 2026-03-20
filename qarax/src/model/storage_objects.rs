use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type, types::Json};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::model::storage_pools;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct StorageObject {
    pub id: Uuid,
    pub name: String,
    pub storage_pool_id: Uuid,
    pub object_type: StorageObjectType,
    pub size_bytes: i64,
    pub config: serde_json::Value,
    pub parent_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
pub struct StorageObjectRow {
    pub id: Uuid,
    pub name: String,
    pub storage_pool_id: Uuid,
    pub object_type: StorageObjectType,
    pub size_bytes: i64,
    pub config: Json<serde_json::Value>,
    pub parent_id: Option<Uuid>,
}

impl From<StorageObjectRow> for StorageObject {
    fn from(row: StorageObjectRow) -> Self {
        StorageObject {
            id: row.id,
            name: row.name,
            storage_pool_id: row.storage_pool_id,
            object_type: row.object_type,
            size_bytes: row.size_bytes,
            config: row.config.0,
            parent_id: row.parent_id,
        }
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "storage_object_type")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StorageObjectType {
    Disk,
    Kernel,
    Initrd,
    Iso,
    Snapshot,
    OciImage,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewStorageObject {
    pub name: String,
    /// Storage pool to place this object in. If omitted, a random active pool is chosen.
    pub storage_pool_id: Option<Uuid>,
    pub object_type: StorageObjectType,
    pub size_bytes: i64,

    #[serde(default)]
    pub config: serde_json::Value,

    pub parent_id: Option<Uuid>,
}

pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
) -> Result<Vec<StorageObject>, sqlx::Error> {
    let storage_objects: Vec<StorageObjectRow> = sqlx::query_as!(
        StorageObjectRow,
        r#"
SELECT id,
        name,
        storage_pool_id,
        object_type as "object_type: _",
        size_bytes,
        config as "config: _",
        parent_id as "parent_id?"
FROM storage_objects
WHERE ($1::text IS NULL OR name = $1)
        "#,
        name_filter
    )
    .fetch_all(pool)
    .await?;

    let storage_objects: Vec<StorageObject> = storage_objects
        .into_iter()
        .map(|so: StorageObjectRow| so.into())
        .collect();
    Ok(storage_objects)
}

pub async fn get_batch(pool: &PgPool, ids: &[Uuid]) -> Result<Vec<StorageObject>, sqlx::Error> {
    let rows: Vec<StorageObjectRow> = sqlx::query_as::<_, StorageObjectRow>(
        r#"
SELECT id,
        name,
        storage_pool_id,
        object_type,
        size_bytes,
        config,
        parent_id
FROM storage_objects
WHERE id = ANY($1)
        "#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get(pool: &PgPool, object_id: Uuid) -> Result<StorageObject, sqlx::Error> {
    let storage_object: StorageObjectRow = sqlx::query_as!(
        StorageObjectRow,
        r#"
SELECT id,
        name,
        storage_pool_id,
        object_type as "object_type: _",
        size_bytes,
        config as "config: _",
        parent_id as "parent_id?"
FROM storage_objects
WHERE id = $1
        "#,
        object_id
    )
    .fetch_one(pool)
    .await?;

    Ok(storage_object.into())
}

pub async fn list_by_pool(pool: &PgPool, pool_id: Uuid) -> Result<Vec<StorageObject>, sqlx::Error> {
    let storage_objects: Vec<StorageObjectRow> = sqlx::query_as!(
        StorageObjectRow,
        r#"
SELECT id,
        name,
        storage_pool_id,
        object_type as "object_type: _",
        size_bytes,
        config as "config: _",
        parent_id as "parent_id?"
FROM storage_objects
WHERE storage_pool_id = $1
        "#,
        pool_id
    )
    .fetch_all(pool)
    .await?;

    let storage_objects: Vec<StorageObject> = storage_objects
        .into_iter()
        .map(|so: StorageObjectRow| so.into())
        .collect();
    Ok(storage_objects)
}

pub async fn list_by_type(
    pool: &PgPool,
    object_type: StorageObjectType,
) -> Result<Vec<StorageObject>, sqlx::Error> {
    let storage_objects: Vec<StorageObjectRow> = sqlx::query_as!(
        StorageObjectRow,
        r#"
SELECT id,
        name,
        storage_pool_id,
        object_type as "object_type: _",
        size_bytes,
        config as "config: _",
        parent_id as "parent_id?"
FROM storage_objects
WHERE object_type = $1
        "#,
        object_type as StorageObjectType
    )
    .fetch_all(pool)
    .await?;

    let storage_objects: Vec<StorageObject> = storage_objects
        .into_iter()
        .map(|so: StorageObjectRow| so.into())
        .collect();
    Ok(storage_objects)
}

pub async fn create(pool: &PgPool, new_object: NewStorageObject) -> Result<Uuid, sqlx::Error> {
    // Resolve pool: use the one provided, or pick a random active pool.
    let pool_id = match new_object.storage_pool_id {
        Some(id) => id,
        None => storage_pools::pick_active(pool)
            .await?
            .ok_or_else(|| sqlx::Error::RowNotFound)?,
    };

    let id = Uuid::new_v4();

    // For disk objects on LOCAL/NFS pools, derive the on-disk path from the
    // pool layout rather than requiring the caller to know the node's
    // filesystem structure. The object UUID is used as the filename so the
    // path is always safe — no user-supplied name reaches the filesystem.
    let config = if (new_object.object_type == StorageObjectType::Disk
        || new_object.object_type == StorageObjectType::Snapshot)
        && new_object.config.get("path").is_none()
    {
        if let Ok(storage_pool) = storage_pools::get(pool, pool_id).await {
            let derived_path = match storage_pool.pool_type {
                storage_pools::StoragePoolType::Local => storage_pool
                    .config
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|base| format!("{}/{}", base, id)),
                storage_pools::StoragePoolType::Nfs => {
                    Some(format!("/var/lib/qarax/pools/{}/{}", pool_id, id))
                }
                _ => None,
            };
            if let Some(path) = derived_path {
                serde_json::json!({ "path": path })
            } else {
                new_object.config
            }
        } else {
            new_object.config
        }
    } else {
        new_object.config
    };

    sqlx::query(
        r#"
INSERT INTO storage_objects (id, name, storage_pool_id, object_type, size_bytes, config, parent_id)
VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(&new_object.name)
    .bind(pool_id)
    .bind(new_object.object_type)
    .bind(new_object.size_bytes)
    .bind(config)
    .bind(new_object.parent_id)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, object_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
DELETE FROM storage_objects
WHERE id = $1
        "#,
        object_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_config(
    pool: &PgPool,
    object_id: Uuid,
    config: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE storage_objects SET config = $1 WHERE id = $2")
        .bind(sqlx::types::Json(config))
        .bind(object_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Extract the path from a storage object's config JSONB field.
/// Expected format: {"path": "/var/lib/qarax/images/vmlinux"}
pub fn get_path_from_config(config: &serde_json::Value) -> Option<String> {
    config
        .as_object()?
        .get("path")?
        .as_str()
        .map(|s| s.to_string())
}
