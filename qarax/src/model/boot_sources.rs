use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
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
