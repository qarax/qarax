use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{model::sandbox_pool_members::SandboxPoolMemberStatus, pagination::PageRequest};

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct SandboxPool {
    pub id: Uuid,
    pub vm_template_id: Uuid,
    pub vm_template_name: String,
    pub min_ready: i32,
    /// The pool makes its members by forking its seed, one VM of the template
    /// it keeps booted, rather than booting each one, and forks a sandbox from
    /// the seed when it has no member ready.
    pub fork: bool,
    /// The seed's VM, while the pool has one.
    pub seed_vm_id: Option<Uuid>,
    pub seed_status: Option<SandboxPoolMemberStatus>,
    /// Counts of the pool's members, not counting its seed.
    pub current_ready: i64,
    pub current_provisioning: i64,
    pub current_error: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct SandboxPoolRow {
    pub id: Uuid,
    pub vm_template_id: Uuid,
    pub vm_template_name: String,
    pub min_ready: i32,
    pub fork: bool,
    pub seed_vm_id: Option<Uuid>,
    pub seed_status: Option<SandboxPoolMemberStatus>,
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
            fork: row.fork,
            seed_vm_id: row.seed_vm_id,
            seed_status: row.seed_status,
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
    /// How many sandboxes the pool keeps ready to hand out.
    pub min_ready: i32,
    /// Make the pool's members by forking one VM of the template it keeps
    /// booted, its seed, rather than booting each one: a member is ready in
    /// milliseconds and shares the seed's memory until it writes it. A
    /// sandbox asked for when the pool has no member ready is forked from the
    /// seed on the request, so with `min_ready` 0 the pool keeps only its seed.
    #[serde(default)]
    pub fork: bool,
}

#[derive(Debug, Clone)]
pub struct SandboxPoolConfig {
    pub id: Uuid,
    pub vm_template_id: Uuid,
    pub min_ready: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Ordered by template name.
pub async fn list(pool: &PgPool, page: &PageRequest) -> Result<Vec<SandboxPool>, sqlx::Error> {
    let rows = sqlx::query_as!(
        SandboxPoolRow,
        r#"
SELECT sp.id,
       sp.vm_template_id,
       vt.name AS vm_template_name,
       sp.min_ready,
       sp.fork,
       seed.vm_id AS "seed_vm_id?",
       seed.status AS "seed_status?: SandboxPoolMemberStatus",
       COUNT(spm.id) FILTER (WHERE spm.status = 'READY')::bigint AS "current_ready!",
       COUNT(spm.id) FILTER (WHERE spm.status = 'PROVISIONING')::bigint AS "current_provisioning!",
       COUNT(spm.id) FILTER (WHERE spm.status = 'ERROR')::bigint AS "current_error!",
       sp.created_at,
       sp.updated_at
FROM sandbox_pools sp
JOIN vm_templates vt
  ON vt.id = sp.vm_template_id
LEFT JOIN sandbox_pool_members seed
  ON seed.sandbox_pool_id = sp.id AND seed.seed
LEFT JOIN sandbox_pool_members spm
  ON spm.sandbox_pool_id = sp.id AND NOT spm.seed
WHERE ($1::text IS NULL OR (vt.name, sp.id) > ($1, $2::uuid))
GROUP BY sp.id, vt.name, seed.vm_id, seed.status
ORDER BY vt.name, sp.id
LIMIT $3
        "#,
        page.after_key(),
        page.after_id(),
        page.fetch_limit()
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_by_template(
    pool: &PgPool,
    vm_template_id: Uuid,
) -> Result<SandboxPool, sqlx::Error> {
    let row = sqlx::query_as!(
        SandboxPoolRow,
        r#"
SELECT sp.id,
       sp.vm_template_id,
       vt.name AS vm_template_name,
       sp.min_ready,
       sp.fork,
       seed.vm_id AS "seed_vm_id?",
       seed.status AS "seed_status?: SandboxPoolMemberStatus",
       COUNT(spm.id) FILTER (WHERE spm.status = 'READY')::bigint AS "current_ready!",
       COUNT(spm.id) FILTER (WHERE spm.status = 'PROVISIONING')::bigint AS "current_provisioning!",
       COUNT(spm.id) FILTER (WHERE spm.status = 'ERROR')::bigint AS "current_error!",
       sp.created_at,
       sp.updated_at
FROM sandbox_pools sp
JOIN vm_templates vt
  ON vt.id = sp.vm_template_id
LEFT JOIN sandbox_pool_members seed
  ON seed.sandbox_pool_id = sp.id AND seed.seed
LEFT JOIN sandbox_pool_members spm
  ON spm.sandbox_pool_id = sp.id AND NOT spm.seed
WHERE sp.vm_template_id = $1
GROUP BY sp.id, vt.name, seed.vm_id, seed.status
        "#,
        vm_template_id
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn get_config(
    pool: &PgPool,
    vm_template_id: Uuid,
) -> Result<Option<SandboxPoolConfig>, sqlx::Error> {
    sqlx::query_as!(
        SandboxPoolConfig,
        r#"
SELECT id, vm_template_id, min_ready, created_at, updated_at
FROM sandbox_pools
WHERE vm_template_id = $1
        "#,
        vm_template_id
    )
    .fetch_optional(pool)
    .await
}

pub async fn upsert(
    pool: &PgPool,
    vm_template_id: Uuid,
    request: &ConfigureSandboxPoolRequest,
) -> Result<SandboxPool, sqlx::Error> {
    sqlx::query!(
        r#"
INSERT INTO sandbox_pools (vm_template_id, min_ready, fork)
VALUES ($1, $2, $3)
ON CONFLICT (vm_template_id)
DO UPDATE SET min_ready = EXCLUDED.min_ready,
              fork = EXCLUDED.fork,
              updated_at = NOW()
        "#,
        vm_template_id,
        request.min_ready,
        request.fork
    )
    .execute(pool)
    .await?;

    get_by_template(pool, vm_template_id).await
}

pub async fn delete_tx(
    tx: &mut Transaction<'_, Postgres>,
    vm_template_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM sandbox_pools WHERE vm_template_id = $1",
        vm_template_id
    )
    .execute(tx.as_mut())
    .await?;
    Ok(())
}
