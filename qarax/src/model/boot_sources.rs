use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use super::storage_objects;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct BootSource {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub kernel_image_id: Uuid,
    pub kernel_params: Option<String>,
    pub initrd_image_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
pub struct BootSourceRow {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub kernel_image_id: Uuid,
    pub kernel_params: Option<String>,
    pub initrd_image_id: Option<Uuid>,
}

impl From<BootSourceRow> for BootSource {
    fn from(row: BootSourceRow) -> Self {
        BootSource {
            id: row.id,
            name: row.name,
            description: row.description,
            kernel_image_id: row.kernel_image_id,
            kernel_params: row.kernel_params,
            initrd_image_id: row.initrd_image_id,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewBootSource {
    pub name: String,
    pub description: Option<String>,
    pub kernel_image_id: Uuid,
    pub kernel_params: Option<String>,
    pub initrd_image_id: Option<Uuid>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<BootSource>, sqlx::Error> {
    let boot_sources: Vec<BootSourceRow> = sqlx::query_as!(
        BootSourceRow,
        r#"
SELECT id,
        name,
        description as "description?",
        kernel_image_id,
        kernel_params as "kernel_params?",
        initrd_image_id as "initrd_image_id?"
FROM boot_sources
        "#
    )
    .fetch_all(pool)
    .await?;

    let boot_sources: Vec<BootSource> = boot_sources
        .into_iter()
        .map(|bs: BootSourceRow| bs.into())
        .collect();
    Ok(boot_sources)
}

pub async fn get(pool: &PgPool, boot_source_id: Uuid) -> Result<BootSource, sqlx::Error> {
    let boot_source: BootSourceRow = sqlx::query_as!(
        BootSourceRow,
        r#"
SELECT id,
        name,
        description as "description?",
        kernel_image_id,
        kernel_params as "kernel_params?",
        initrd_image_id as "initrd_image_id?"
FROM boot_sources
WHERE id = $1
        "#,
        boot_source_id
    )
    .fetch_one(pool)
    .await?;

    Ok(boot_source.into())
}

pub async fn get_by_name(pool: &PgPool, name: &str) -> Result<BootSource, sqlx::Error> {
    let boot_source: BootSourceRow = sqlx::query_as!(
        BootSourceRow,
        r#"
SELECT id,
        name,
        description as "description?",
        kernel_image_id,
        kernel_params as "kernel_params?",
        initrd_image_id as "initrd_image_id?"
FROM boot_sources
WHERE name = $1
        "#,
        name
    )
    .fetch_one(pool)
    .await?;

    Ok(boot_source.into())
}

pub async fn create(pool: &PgPool, new_boot_source: NewBootSource) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query!(
        r#"
INSERT INTO boot_sources (id, name, description, kernel_image_id, kernel_params, initrd_image_id)
VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id,
        new_boot_source.name,
        new_boot_source.description,
        new_boot_source.kernel_image_id,
        new_boot_source.kernel_params,
        new_boot_source.initrd_image_id
    )
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, boot_source_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
DELETE FROM boot_sources
WHERE id = $1
        "#,
        boot_source_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Resolved boot source with actual file paths
#[derive(Debug, Clone)]
pub struct ResolvedBootSource {
    pub kernel_path: String,
    pub kernel_params: String,
    pub initramfs_path: Option<String>,
}

/// Resolve a boot source by fetching the storage objects and extracting their paths
pub async fn resolve(
    pool: &PgPool,
    boot_source_id: Uuid,
) -> Result<ResolvedBootSource, sqlx::Error> {
    let boot_source = get(pool, boot_source_id).await?;

    // Fetch kernel storage object
    let kernel_obj = storage_objects::get(pool, boot_source.kernel_image_id).await?;
    let kernel_path = storage_objects::get_path_from_config(&kernel_obj.config)
        .ok_or_else(|| sqlx::Error::RowNotFound)?;

    // Fetch initramfs storage object if present
    let initramfs_path = if let Some(initrd_id) = boot_source.initrd_image_id {
        let initrd_obj = storage_objects::get(pool, initrd_id).await?;
        storage_objects::get_path_from_config(&initrd_obj.config)
    } else {
        None
    };

    Ok(ResolvedBootSource {
        kernel_path,
        kernel_params: boot_source.kernel_params.unwrap_or_default(),
        initramfs_path,
    })
}
