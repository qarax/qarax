use std::collections::HashMap;

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
    /// The pool makes its members by forking its seeds, VMs of the template
    /// it keeps booted, rather than booting each one, and forks a sandbox from
    /// a seed when it has no member ready.
    pub fork: bool,
    /// How many hosts the pool keeps a seed on, when it forks.
    pub seed_hosts: i32,
    /// The pool's seeds, oldest first: at most one on a host.
    pub seeds: Vec<SandboxPoolSeed>,
    /// Counts of the pool's members, not counting its seeds.
    pub current_ready: i64,
    pub current_provisioning: i64,
    pub current_error: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A VM of the template a forking pool keeps booted and forks from.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct SandboxPoolSeed {
    pub vm_id: Uuid,
    /// The host it runs on.
    pub host_id: Option<Uuid>,
    pub status: SandboxPoolMemberStatus,
}

#[derive(Debug, Clone)]
struct SandboxPoolRow {
    pub id: Uuid,
    pub vm_template_id: Uuid,
    pub vm_template_name: String,
    pub min_ready: i32,
    pub fork: bool,
    pub seed_hosts: i32,
    pub current_ready: i64,
    pub current_provisioning: i64,
    pub current_error: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SandboxPoolRow {
    fn with_seeds(self, seeds: Vec<SandboxPoolSeed>) -> SandboxPool {
        SandboxPool {
            id: self.id,
            vm_template_id: self.vm_template_id,
            vm_template_name: self.vm_template_name,
            min_ready: self.min_ready,
            fork: self.fork,
            seed_hosts: self.seed_hosts,
            seeds,
            current_ready: self.current_ready,
            current_provisioning: self.current_provisioning,
            current_error: self.current_error,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

struct SeedRow {
    sandbox_pool_id: Uuid,
    vm_id: Uuid,
    host_id: Option<Uuid>,
    status: SandboxPoolMemberStatus,
}

/// Each row's pool, with its seeds.
async fn with_seeds(
    pool: &PgPool,
    rows: Vec<SandboxPoolRow>,
) -> Result<Vec<SandboxPool>, sqlx::Error> {
    let pool_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let seeds = sqlx::query_as!(
        SeedRow,
        r#"
SELECT seed.sandbox_pool_id,
       seed.vm_id,
       v.host_id,
       seed.status AS "status: SandboxPoolMemberStatus"
FROM sandbox_pool_members seed
JOIN vms v
  ON v.id = seed.vm_id
WHERE seed.sandbox_pool_id = ANY($1)
  AND seed.seed
ORDER BY seed.created_at, seed.id
        "#,
        &pool_ids
    )
    .fetch_all(pool)
    .await?;
    let mut by_pool: HashMap<Uuid, Vec<SandboxPoolSeed>> = HashMap::new();
    for seed in seeds {
        by_pool
            .entry(seed.sandbox_pool_id)
            .or_default()
            .push(SandboxPoolSeed {
                vm_id: seed.vm_id,
                host_id: seed.host_id,
                status: seed.status,
            });
    }
    Ok(rows
        .into_iter()
        .map(|row| {
            // A pool with no seed has no entry.
            let seeds = by_pool.remove(&row.id).unwrap_or_default();
            row.with_seeds(seeds)
        })
        .collect())
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct ConfigureSandboxPoolRequest {
    /// How many sandboxes the pool keeps ready to hand out.
    pub min_ready: i32,
    /// Make the pool's members by forking a VM of the template it keeps
    /// booted, a seed, rather than booting each one: a member is ready in
    /// milliseconds and shares the seed's memory until it writes it. A
    /// sandbox asked for when the pool has no member ready is forked from a
    /// seed on the request, so with `min_ready` 0 the pool keeps only seeds.
    #[serde(default)]
    pub fork: bool,
    /// For a pool that forks: how many hosts to keep a seed on, one each. A
    /// member or a sandbox is forked from the seed whose host holds the fewest
    /// of the pool's members, or the next when that host has no room. 1 if
    /// unset.
    #[serde(default = "one_seed_host")]
    #[schema(default = 1, minimum = 1)]
    pub seed_hosts: i32,
}

fn one_seed_host() -> i32 {
    1
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
       sp.seed_hosts,
       COUNT(spm.id) FILTER (WHERE spm.status = 'READY')::bigint AS "current_ready!",
       COUNT(spm.id) FILTER (WHERE spm.status = 'PROVISIONING')::bigint AS "current_provisioning!",
       COUNT(spm.id) FILTER (WHERE spm.status = 'ERROR')::bigint AS "current_error!",
       sp.created_at,
       sp.updated_at
FROM sandbox_pools sp
JOIN vm_templates vt
  ON vt.id = sp.vm_template_id
LEFT JOIN sandbox_pool_members spm
  ON spm.sandbox_pool_id = sp.id AND NOT spm.seed
WHERE ($1::text IS NULL OR (vt.name, sp.id) > ($1, $2::uuid))
GROUP BY sp.id, vt.name
ORDER BY vt.name, sp.id
LIMIT $3
        "#,
        page.after_key(),
        page.after_id(),
        page.fetch_limit()
    )
    .fetch_all(pool)
    .await?;

    with_seeds(pool, rows).await
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
       sp.seed_hosts,
       COUNT(spm.id) FILTER (WHERE spm.status = 'READY')::bigint AS "current_ready!",
       COUNT(spm.id) FILTER (WHERE spm.status = 'PROVISIONING')::bigint AS "current_provisioning!",
       COUNT(spm.id) FILTER (WHERE spm.status = 'ERROR')::bigint AS "current_error!",
       sp.created_at,
       sp.updated_at
FROM sandbox_pools sp
JOIN vm_templates vt
  ON vt.id = sp.vm_template_id
LEFT JOIN sandbox_pool_members spm
  ON spm.sandbox_pool_id = sp.id AND NOT spm.seed
WHERE sp.vm_template_id = $1
GROUP BY sp.id, vt.name
        "#,
        vm_template_id
    )
    .fetch_one(pool)
    .await?;

    Ok(with_seeds(pool, vec![row]).await?.remove(0))
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
INSERT INTO sandbox_pools (vm_template_id, min_ready, fork, seed_hosts)
VALUES ($1, $2, $3, $4)
ON CONFLICT (vm_template_id)
DO UPDATE SET min_ready = EXCLUDED.min_ready,
              fork = EXCLUDED.fork,
              seed_hosts = EXCLUDED.seed_hosts,
              updated_at = NOW()
        "#,
        vm_template_id,
        request.min_ready,
        request.fork,
        request.seed_hosts
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
