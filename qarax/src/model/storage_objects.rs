use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type, types::Json};
use strum_macros::{Display, EnumString};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
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
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewStorageObject {
    pub name: String,
    pub storage_pool_id: Uuid,
    pub object_type: StorageObjectType,
    pub size_bytes: i64,

    #[serde(default)]
    pub config: serde_json::Value,

    pub parent_id: Option<Uuid>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<StorageObject>, sqlx::Error> {
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
        "#
    )
    .fetch_all(pool)
    .await?;

    let storage_objects: Vec<StorageObject> = storage_objects
        .into_iter()
        .map(|so: StorageObjectRow| so.into())
        .collect();
    Ok(storage_objects)
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
