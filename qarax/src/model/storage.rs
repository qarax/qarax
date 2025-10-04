use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Type, types::Json};
use strum_macros::{Display, EnumString};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct Storage {
    pub id: Uuid,
    pub name: String,
    pub status: StorageStatus,
    pub storage_type: StorageType,
    pub config: StorageConfig,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "storage_type")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StorageType {
    Local,
    Shared,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "storage_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StorageStatus {
    Up,
    Down,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StorageName(pub String);

pub struct NewStorage {
    pub name: StorageName,
    pub storage_type: StorageType,
    pub config: StorageConfig,
}

#[derive(Deserialize, Serialize)]
pub struct StorageConfig {
    pub host_id: Option<Uuid>,
    pub path_on_host: Option<String>,
    pub pool_name: Option<String>,
}

#[derive(Serialize, Deserialize, FromRow)]
struct StorageRow {
    id: Uuid,
    name: String,
    status: StorageStatus,
    storage_type: StorageType,
    config: Json<StorageConfig>,
}

impl From<StorageRow> for Storage {
    fn from(row: StorageRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            status: row.status,
            storage_type: row.storage_type,
            config: StorageConfig {
                host_id: row.config.host_id,
                path_on_host: row.config.path_on_host.to_owned(),
                pool_name: row.config.pool_name.to_owned(),
            },
        }
    }
}

pub async fn list(pool: &PgPool) -> Result<Vec<Storage>, sqlx::Error> {
    let storages = sqlx::query_as!(
        StorageRow,
        r#"
        SELECT id, 
               name, 
               status as "status: _", 
               storage_type as "storage_type: _", 
               config as "config: _"
        FROM storages
        "#,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Storage::from)
    .collect();

    Ok(storages)
}
