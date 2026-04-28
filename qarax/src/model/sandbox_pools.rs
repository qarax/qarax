use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct SandboxPool {
    pub id: Uuid,
    pub vm_template_id: Uuid,
    pub vm_template_name: String,
    pub min_ready: i32,
    pub current_ready: i64,
    pub current_provisioning: i64,
    pub current_error: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
struct SandboxPoolRow {
    pub id: Uuid,
    pub vm_template_id: Uuid,
    pub vm_template_name: String,
    pub min_ready: i32,
    pub current_ready: i64,
    pub current_provisioning: i64,
    pub current_error: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<SandboxPoolRow> for SandboxPool {
    fn from(row: SandboxPoolRow) -> Self {
        Self {
            id: row.id,
            vm_template_id: row.vm_template_id,
            vm_template_name: row.vm_template_name,
            min_ready: row.min_ready,
            current_ready: row.current_ready,
            current_provisioning: row.current_provisioning,
            current_error: row.current_error,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct ConfigureSandboxPoolRequest {
    pub min_ready: i32,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct SandboxPoolConfig {
    pub id: Uuid,
    pub vm_template_id: Uuid,
    pub min_ready: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<SandboxPool>, sqlx::Error> {
    let rows = sqlx::query_as::<_, SandboxPoolRow>(
        r#"
SELECT sp.id,
       sp.vm_template_id,
       vt.name AS vm_template_name,
       sp.min_ready,
       COUNT(*) FILTER (WHERE spm.status = 'READY')::bigint AS current_ready,
       COUNT(*) FILTER (WHERE spm.status = 'PROVISIONING')::bigint AS current_provisioning,
       COUNT(*) FILTER (WHERE spm.status = 'ERROR')::bigint AS current_error,
       sp.created_at,
       sp.updated_at
FROM sandbox_pools sp
JOIN vm_templates vt
  ON vt.id = sp.vm_template_id
LEFT JOIN sandbox_pool_members spm
  ON spm.sandbox_pool_id = sp.id
GROUP BY sp.id, vt.name
ORDER BY vt.name
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_by_template(
    pool: &PgPool,
    vm_template_id: Uuid,
) -> Result<SandboxPool, sqlx::Error> {
    let row = sqlx::query_as::<_, SandboxPoolRow>(
        r#"
SELECT sp.id,
       sp.vm_template_id,
       vt.name AS vm_template_name,
       sp.min_ready,
       COUNT(*) FILTER (WHERE spm.status = 'READY')::bigint AS current_ready,
       COUNT(*) FILTER (WHERE spm.status = 'PROVISIONING')::bigint AS current_provisioning,
       COUNT(*) FILTER (WHERE spm.status = 'ERROR')::bigint AS current_error,
       sp.created_at,
       sp.updated_at
FROM sandbox_pools sp
JOIN vm_templates vt
  ON vt.id = sp.vm_template_id
LEFT JOIN sandbox_pool_members spm
  ON spm.sandbox_pool_id = sp.id
WHERE sp.vm_template_id = $1
GROUP BY sp.id, vt.name
        "#,
    )
    .bind(vm_template_id)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn get_config(
    pool: &PgPool,
    vm_template_id: Uuid,
) -> Result<Option<SandboxPoolConfig>, sqlx::Error> {
    sqlx::query_as::<_, SandboxPoolConfig>(
        r#"
SELECT id, vm_template_id, min_ready, created_at, updated_at
FROM sandbox_pools
WHERE vm_template_id = $1
        "#,
    )
    .bind(vm_template_id)
    .fetch_optional(pool)
    .await
}

pub async fn upsert(
    pool: &PgPool,
    vm_template_id: Uuid,
    min_ready: i32,
) -> Result<SandboxPool, sqlx::Error> {
    sqlx::query(
        r#"
INSERT INTO sandbox_pools (vm_template_id, min_ready)
VALUES ($1, $2)
ON CONFLICT (vm_template_id)
DO UPDATE SET min_ready = EXCLUDED.min_ready,
              updated_at = NOW()
        "#,
    )
    .bind(vm_template_id)
    .bind(min_ready)
    .execute(pool)
    .await?;

    get_by_template(pool, vm_template_id).await
}

pub async fn delete_tx(
    tx: &mut Transaction<'_, Postgres>,
    vm_template_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sandbox_pools WHERE vm_template_id = $1")
        .bind(vm_template_id)
        .execute(tx.as_mut())
        .await?;
    Ok(())
}
