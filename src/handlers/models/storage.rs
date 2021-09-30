use sqlx::types::Json;

use super::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Storage {
    pub id: Uuid,
    pub name: String,
    pub status: Status,
    pub storage_type: StorageType,
    pub config: Json<StorageConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewStorage {
    pub name: String,
    pub storage_type: StorageType,
    pub config: StorageConfig,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[serde(rename_all = "lowercase")]
pub enum StorageType {
    #[strum(serialize = "local")]
    Local,
    #[strum(serialize = "shared")]
    Shared,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[strum(serialize = "up")]
    Up,
    #[strum(serialize = "down")]
    Down,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct StorageConfig {
    pub host_id: Option<Uuid>,
    pub path: Option<String>,
    pub pool_name: Option<String>,
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Couldn't list storages: '{0}'")]
    List(sqlx::Error),

    #[error("Couldn't find storage: '{0}', error: '{1}'")]
    Find(Uuid, sqlx::Error),

    #[error("Couldn't add storage '{0}', error: '{1}'")]
    Add(String, sqlx::Error),
}

pub async fn list(pool: &PgPool) -> Result<Vec<Storage>, StorageError> {
    let storages = sqlx::query_as!(
        Storage,
        r#"
SELECT id, name, status as "status: _", storage_type as "storage_type: _", config as "config: Json<StorageConfig>" 
FROM storage
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(StorageError::List)?;

    Ok(storages)
}

pub async fn add(pool: &PgPool, storage: &NewStorage) -> Result<Uuid, StorageError> {
    let rec = sqlx::query!(
        r#"
INSERT INTO storage (name, status, storage_type, config)
VALUES ( $1, $2, $3, $4)
RETURNING id
        "#,
        storage.name,
        Status::Down as Status,
        storage.storage_type.to_string(),
        Json(&storage.config) as _,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| StorageError::Add(storage.name.to_owned(), e))?;

    Ok(rec.id)
}

pub async fn by_id(pool: &PgPool, storage_id: Uuid) -> Result<Storage, StorageError> {
    let storage = sqlx::query_as!(
        Storage,
        r#"
SELECT id, name, status as "status: _", storage_type as "storage_type: _", config as "config: Json<StorageConfig>" 
FROM storage
WHERE id = $1
        "#,
        storage_id
    )
    .fetch_one(pool)
    .await
    .map_err(|e| StorageError::Find(storage_id, e))?;

    Ok(storage)
}
