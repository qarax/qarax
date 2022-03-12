use sqlx::types::Json;

use crate::handlers::ServerError;
use lazy_static::lazy_static;
use regex::Regex;

use super::*;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Inavlid name: {0}")]
    InvalidName(String),
    #[error("Invalid config {0}")]
    InvalidConfig(String),
    #[error("Unexpected failure: {0}")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<sqlx::Error> for StorageError {
    fn from(e: sqlx::Error) -> Self {
        StorageError::Other(Box::new(e))
    }
}

impl From<StorageError> for ServerError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::InvalidName(e) => ServerError::Validation(format!("Invalid name {e}")),
            StorageError::InvalidConfig(e) => {
                ServerError::Validation(format!("Invalid config {e}"))
            }
            StorageError::Other(e) => ServerError::Internal(e.to_string()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StorageName(pub String);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewStorage {
    pub name: StorageName,
    pub storage_type: StorageType,
    pub config: StorageConfig,
}

impl AsRef<str> for StorageName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl StorageName {
    pub fn new(name: String) -> Result<Self, StorageError> {
        lazy_static! {
            static ref RE: Regex = Regex::new("^[a-zA-Z0-9-_]+$").unwrap();
        }
        if !RE.is_match(&name) {
            return Err(StorageError::InvalidName(name));
        }

        Ok(Self(name))
    }
}

impl NewStorage {
    pub fn new(
        name: StorageName,
        storage_type: StorageType,
        config: StorageConfig,
    ) -> Result<Self, StorageError> {
        match storage_type {
            StorageType::Local => {
                if config.host_id.is_none() {
                    return Err(StorageError::InvalidConfig(String::from(
                        "missing host_id for local storage",
                    )));
                }
            }
            StorageType::Shared => {
                if config.pool_name.is_none() {
                    return Err(StorageError::InvalidConfig(String::from(
                        "missing pool name for shared storage",
                    )));
                }
            }
        }

        Ok(Self {
            name,
            storage_type,
            config,
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Storage {
    pub id: Uuid,
    pub name: String,
    pub status: Status,
    pub storage_type: StorageType,
    pub config: Json<StorageConfig>,
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
    pub pool_name: Option<String>,
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
    .await?;

    Ok(storages)
}

pub async fn add(pool: &PgPool, storage: &NewStorage) -> Result<Uuid, StorageError> {
    let rec = sqlx::query!(
        r#"
INSERT INTO storage (name, status, storage_type, config)
VALUES ( $1, $2, $3, $4)
RETURNING id
        "#,
        storage.name.0,
        Status::Down as Status,
        storage.storage_type.to_string(),
        Json(&storage.config) as _,
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn by_id(pool: &PgPool, storage_id: &Uuid) -> Result<Storage, StorageError> {
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
    .await?;

    Ok(storage)
}
