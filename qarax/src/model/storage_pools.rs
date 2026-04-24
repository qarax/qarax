use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type, types::Json};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

/// Configuration for an OverlayBD storage pool, extracted from the JSONB `config` column.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OverlayBdPoolConfig {
    pub url: String,
}

impl OverlayBdPoolConfig {
    pub fn from_value(v: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(v.clone()).ok()
    }
}

/// Configuration for a BLOCK (iSCSI) storage pool, extracted from the JSONB `config` column.
///
/// `portal` is the iSCSI target portal (host:port, e.g. `10.0.0.5:3260`).
/// `iqn` is the target IQN (e.g. `iqn.2024-01.qarax:target0`).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlockPoolConfig {
    pub portal: String,
    pub iqn: String,
}

impl BlockPoolConfig {
    pub fn from_value(v: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(v.clone()).ok()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct StoragePool {
    pub id: Uuid,
    pub name: String,
    pub pool_type: StoragePoolType,
    pub status: StoragePoolStatus,
    pub config: serde_json::Value,
    pub capacity_bytes: Option<i64>,
    pub allocated_bytes: Option<i64>,
}

#[derive(sqlx::FromRow)]
pub struct StoragePoolRow {
    pub id: Uuid,
    pub name: String,
    pub pool_type: StoragePoolType,
    pub status: StoragePoolStatus,
    pub config: Json<serde_json::Value>,
    pub capacity_bytes: Option<i64>,
    pub allocated_bytes: Option<i64>,
}

impl From<StoragePoolRow> for StoragePool {
    fn from(row: StoragePoolRow) -> Self {
        StoragePool {
            id: row.id,
            name: row.name,
            pool_type: row.pool_type,
            status: row.status,
            config: row.config.0,
            capacity_bytes: row.capacity_bytes,
            allocated_bytes: row.allocated_bytes,
        }
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "storage_pool_type")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StoragePoolType {
    Local,
    Nfs,
    #[sqlx(rename = "OVERLAYBD")]
    #[serde(alias = "overlaybd")]
    OverlayBd,
    Block,
}

impl StoragePoolType {
    /// Whether this pool type represents shared storage accessible from multiple hosts.
    ///
    /// NFS pools are accessible from multiple hosts at the same path.
    /// OverlayBD pools are backed by a shared OCI registry.
    /// BLOCK pools are iSCSI targets, reachable from any initiator on the network.
    /// Local pools are host-specific and must be attached explicitly.
    pub fn is_shared(&self) -> bool {
        matches!(
            self,
            StoragePoolType::Nfs | StoragePoolType::OverlayBd | StoragePoolType::Block
        )
    }

    /// Whether VMs using this pool type can be live-migrated.
    pub fn supports_live_migration(&self) -> bool {
        self.is_shared()
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "storage_pool_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StoragePoolStatus {
    Active,
    Inactive,
    Error,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewStoragePool {
    pub name: String,
    pub pool_type: StoragePoolType,

    #[serde(default)]
    pub config: serde_json::Value,

    pub capacity_bytes: Option<i64>,
}

pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
) -> Result<Vec<StoragePool>, sqlx::Error> {
    let rows: Vec<StoragePoolRow> = sqlx::query_as::<_, StoragePoolRow>(
        r#"
SELECT id, name, pool_type, status, config, capacity_bytes, allocated_bytes
FROM storage_pools
WHERE ($1::text IS NULL OR name = $1)
        "#,
    )
    .bind(name_filter)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get(pool: &PgPool, pool_id: Uuid) -> Result<StoragePool, sqlx::Error> {
    let row: StoragePoolRow = sqlx::query_as::<_, StoragePoolRow>(
        r#"
SELECT id, name, pool_type, status, config, capacity_bytes, allocated_bytes
FROM storage_pools
WHERE id = $1
        "#,
    )
    .bind(pool_id)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn get_batch(pool: &PgPool, ids: &[Uuid]) -> Result<Vec<StoragePool>, sqlx::Error> {
    let rows: Vec<StoragePoolRow> = sqlx::query_as::<_, StoragePoolRow>(
        r#"
SELECT id, name, pool_type, status, config, capacity_bytes, allocated_bytes
FROM storage_pools
WHERE id = ANY($1)
        "#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn create(pool: &PgPool, new_pool: NewStoragePool) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query(
        r#"
INSERT INTO storage_pools (id, name, pool_type, status, config, capacity_bytes)
VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(&new_pool.name)
    .bind(new_pool.pool_type)
    .bind(StoragePoolStatus::Active)
    .bind(new_pool.config)
    .bind(new_pool.capacity_bytes)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, pool_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM storage_pools WHERE id = $1")
        .bind(pool_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Return any active non-OverlayBD pool. If `prefer_pool_id` is given and it
/// is active and not OverlayBD, it is returned directly; otherwise a random
/// qualifying pool is chosen.
pub async fn pick_active_non_overlaybd(
    pool: &PgPool,
    prefer_pool_id: Option<Uuid>,
) -> Result<Option<Uuid>, sqlx::Error> {
    if let Some(id) = prefer_pool_id {
        let row = sqlx::query_as::<_, (Uuid,)>(
            "SELECT id FROM storage_pools WHERE id = $1 AND status = 'ACTIVE' AND pool_type != 'OVERLAYBD'",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        if let Some((found_id,)) = row {
            return Ok(Some(found_id));
        }
    }
    let row = sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM storage_pools WHERE status = 'ACTIVE' AND pool_type != 'OVERLAYBD' ORDER BY RANDOM() LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id,)| id))
}

/// Return a random active storage pool ID (used when none is specified for disk creation).
pub async fn pick_active(pool: &PgPool) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM storage_pools WHERE status = 'ACTIVE' ORDER BY RANDOM() LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id,)| id))
}

/// Attach a host to a storage pool (idempotent).
pub async fn attach_host(pool: &PgPool, pool_id: Uuid, host_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
INSERT INTO host_storage_pools (host_id, storage_pool_id)
VALUES ($1, $2)
ON CONFLICT DO NOTHING
        "#,
    )
    .bind(host_id)
    .bind(pool_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Detach a host from a storage pool.
pub async fn detach_host(pool: &PgPool, pool_id: Uuid, host_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM host_storage_pools WHERE host_id = $1 AND storage_pool_id = $2")
        .bind(host_id)
        .bind(pool_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// List all storage pools attached to a specific host.
pub async fn list_for_host(pool: &PgPool, host_id: Uuid) -> Result<Vec<StoragePool>, sqlx::Error> {
    let rows: Vec<StoragePoolRow> = sqlx::query_as::<_, StoragePoolRow>(
        r#"
SELECT sp.id, sp.name, sp.pool_type, sp.status, sp.config, sp.capacity_bytes, sp.allocated_bytes
FROM storage_pools sp
JOIN host_storage_pools hsp ON hsp.storage_pool_id = sp.id
WHERE hsp.host_id = $1
        "#,
    )
    .bind(host_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn find_host_for_pool(pool: &PgPool, pool_id: Uuid) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query_as::<_, (Uuid,)>(
        "SELECT hsp.host_id FROM host_storage_pools hsp \
         JOIN hosts h ON h.id = hsp.host_id \
         WHERE hsp.storage_pool_id = $1 \
         ORDER BY CASE WHEN h.status = 'UP' THEN 0 ELSE 1 END, h.load_average ASC NULLS LAST \
         LIMIT 1",
    )
    .bind(pool_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id,)| id))
}

/// Check whether a host is attached to a given storage pool.
pub async fn host_has_pool(
    pool: &PgPool,
    host_id: Uuid,
    pool_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM host_storage_pools WHERE host_id = $1 AND storage_pool_id = $2",
    )
    .bind(host_id)
    .bind(pool_id)
    .fetch_one(pool)
    .await?;

    Ok(row.0 > 0)
}

/// Return the first active OverlayBD storage pool that the given host is attached to, if any.
pub async fn find_overlaybd_for_host(
    pool: &PgPool,
    host_id: Uuid,
) -> Result<Option<StoragePool>, sqlx::Error> {
    let row = sqlx::query_as::<_, StoragePoolRow>(
        r#"
SELECT sp.id, sp.name, sp.pool_type, sp.status, sp.config, sp.capacity_bytes, sp.allocated_bytes
FROM storage_pools sp
JOIN host_storage_pools hsp ON hsp.storage_pool_id = sp.id
WHERE hsp.host_id = $1
  AND sp.pool_type = 'OVERLAYBD'
  AND sp.status = 'ACTIVE'
ORDER BY sp.name
LIMIT 1
        "#,
    )
    .bind(host_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into()))
}
