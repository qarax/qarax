use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::model::vms::VmStatus;

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "sandbox_pool_member_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SandboxPoolMemberStatus {
    Provisioning,
    Ready,
    Error,
    Destroying,
}

#[derive(Debug, Clone)]
pub struct SandboxPoolMember {
    pub id: Uuid,
    pub sandbox_pool_id: Uuid,
    pub vm_id: Uuid,
    pub status: SandboxPoolMemberStatus,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A PROVISIONING member of the pool: one of its seeds, which are never
/// claimed, or a member a sandbox claims.
pub async fn create(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
    vm_id: Uuid,
    seed: bool,
) -> Result<SandboxPoolMember, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let id = create_tx(&mut tx, sandbox_pool_id, vm_id, seed).await?;
    tx.commit().await?;
    get(pool, id).await
}

/// [`create`] inside the caller's transaction.
pub async fn create_tx(
    tx: &mut Transaction<'_, Postgres>,
    sandbox_pool_id: Uuid,
    vm_id: Uuid,
    seed: bool,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query!(
        r#"
INSERT INTO sandbox_pool_members (id, sandbox_pool_id, vm_id, seed)
VALUES ($1, $2, $3, $4)
        "#,
        id,
        sandbox_pool_id,
        vm_id,
        seed
    )
    .execute(tx.as_mut())
    .await?;
    Ok(id)
}

/// A seed of a pool, with where its VM runs.
#[derive(Debug, Clone)]
pub struct Seed {
    pub member: SandboxPoolMember,
    pub vm_status: VmStatus,
    pub host_id: Option<Uuid>,
    /// Whether it has a host and the host is up: only then can it fork, or be
    /// destroyed.
    pub host_up: bool,
}

struct SeedRow {
    id: Uuid,
    sandbox_pool_id: Uuid,
    vm_id: Uuid,
    status: SandboxPoolMemberStatus,
    error_message: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    vm_status: VmStatus,
    host_id: Option<Uuid>,
    host_up: bool,
}

/// The pool's seeds, oldest first.
pub async fn seeds(pool: &PgPool, sandbox_pool_id: Uuid) -> Result<Vec<Seed>, sqlx::Error> {
    let rows = sqlx::query_as!(
        SeedRow,
        r#"
SELECT m.id,
       m.sandbox_pool_id,
       m.vm_id,
       m.status AS "status: _",
       m.error_message,
       m.created_at,
       m.updated_at,
       v.status AS "vm_status: _",
       v.host_id,
       COALESCE(h.status = 'UP', false) AS "host_up!"
FROM sandbox_pool_members m
JOIN vms v
  ON v.id = m.vm_id
LEFT JOIN hosts h
  ON h.id = v.host_id
WHERE m.sandbox_pool_id = $1 AND m.seed
ORDER BY m.created_at, m.id
        "#,
        sandbox_pool_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Seed {
            member: SandboxPoolMember {
                id: row.id,
                sandbox_pool_id: row.sandbox_pool_id,
                vm_id: row.vm_id,
                status: row.status,
                error_message: row.error_message,
                created_at: row.created_at,
                updated_at: row.updated_at,
            },
            vm_status: row.vm_status,
            host_id: row.host_id,
            host_up: row.host_up,
        })
        .collect())
}

pub async fn get(pool: &PgPool, member_id: Uuid) -> Result<SandboxPoolMember, sqlx::Error> {
    sqlx::query_as!(
        SandboxPoolMember,
        r#"
SELECT id, sandbox_pool_id, vm_id, status AS "status: _", error_message, created_at, updated_at
FROM sandbox_pool_members
WHERE id = $1
        "#,
        member_id
    )
    .fetch_one(pool)
    .await
}

/// The pool's ready members, oldest first; never its seed.
pub async fn list_ready_by_pool(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
) -> Result<Vec<SandboxPoolMember>, sqlx::Error> {
    sqlx::query_as!(
        SandboxPoolMember,
        r#"
SELECT id, sandbox_pool_id, vm_id, status AS "status: _", error_message, created_at, updated_at
FROM sandbox_pool_members
WHERE sandbox_pool_id = $1
  AND status = 'READY'
  AND NOT seed
ORDER BY created_at
        "#,
        sandbox_pool_id
    )
    .fetch_all(pool)
    .await
}

/// The pool's members that failed, its seed among them.
pub async fn list_error_by_pool(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
) -> Result<Vec<SandboxPoolMember>, sqlx::Error> {
    sqlx::query_as!(
        SandboxPoolMember,
        r#"
SELECT id, sandbox_pool_id, vm_id, status AS "status: _", error_message, created_at, updated_at
FROM sandbox_pool_members
WHERE sandbox_pool_id = $1
  AND status = 'ERROR'
ORDER BY created_at
        "#,
        sandbox_pool_id
    )
    .fetch_all(pool)
    .await
}

/// Every member of the pool, its seed among them.
pub async fn list_by_pool(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
) -> Result<Vec<SandboxPoolMember>, sqlx::Error> {
    sqlx::query_as!(
        SandboxPoolMember,
        r#"
SELECT id, sandbox_pool_id, vm_id, status AS "status: _", error_message, created_at, updated_at
FROM sandbox_pool_members
WHERE sandbox_pool_id = $1
ORDER BY created_at
        "#,
        sandbox_pool_id
    )
    .fetch_all(pool)
    .await
}

/// How many of the pool's members, not counting its seed, are in `status`.
pub async fn count_by_status(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
    status: SandboxPoolMemberStatus,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query_scalar!(
        r#"
SELECT COUNT(*)::bigint AS "count!"
FROM sandbox_pool_members
WHERE sandbox_pool_id = $1
  AND status = $2
  AND NOT seed
        "#,
        sandbox_pool_id,
        status as SandboxPoolMemberStatus
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

// A member moves PROVISIONING -> READY | ERROR, then to DESTROYING (or is
// claimed: its row is deleted and its VM handed to a sandbox).
// `error_message` says why it failed: set with ERROR and kept while the
// member is destroyed. Each write below moves it only from the statuses that
// may precede its own and returns whether it did.

/// PROVISIONING -> READY.
pub async fn mark_ready(pool: &PgPool, member_id: Uuid) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE sandbox_pool_members SET status = 'READY', updated_at = NOW() \
         WHERE id = $1 AND status = 'PROVISIONING'",
        member_id
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

/// PROVISIONING -> ERROR.
pub async fn mark_error(
    pool: &PgPool,
    member_id: Uuid,
    error_message: &str,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE sandbox_pool_members SET status = 'ERROR', error_message = $2, updated_at = NOW() \
         WHERE id = $1 AND status = 'PROVISIONING'",
        member_id,
        error_message
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

/// Any status but DESTROYING -> DESTROYING. `false` means the member is gone
/// (claimed by a sandbox) or someone else is already destroying it: either
/// way its VM is not this caller's to destroy.
pub async fn mark_destroying(pool: &PgPool, member_id: Uuid) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE sandbox_pool_members SET status = 'DESTROYING', updated_at = NOW() \
         WHERE id = $1 AND status <> 'DESTROYING'",
        member_id
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

pub async fn delete(pool: &PgPool, member_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM sandbox_pool_members WHERE id = $1", member_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_tx(
    tx: &mut Transaction<'_, Postgres>,
    member_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM sandbox_pool_members WHERE id = $1", member_id)
        .execute(tx.as_mut())
        .await?;
    Ok(())
}

/// A seed of a forking pool, ready to fork, and how many of the pool's
/// members its host holds, booting or ready.
#[derive(Debug, Clone)]
pub struct ForkableSeed {
    pub vm_id: Uuid,
    pub members: i64,
}

/// The seeds of the template's pool, when it forks, that are ready and run on
/// an up host: those whose host holds the fewest of the pool's members first,
/// then the oldest. Members and sandboxes are forked from them in that order,
/// which spreads the members across the seeds' hosts.
pub async fn forkable_seeds(
    pool: &PgPool,
    vm_template_id: Uuid,
) -> Result<Vec<ForkableSeed>, sqlx::Error> {
    sqlx::query_as!(
        ForkableSeed,
        r#"
SELECT seed.vm_id,
       (SELECT COUNT(*)
        FROM sandbox_pool_members member
        JOIN vms member_vm
          ON member_vm.id = member.vm_id
        WHERE member.sandbox_pool_id = seed.sandbox_pool_id
          AND NOT member.seed
          AND member.status IN ('PROVISIONING', 'READY')
          AND member_vm.host_id = v.host_id) AS "members!"
FROM sandbox_pool_members seed
JOIN sandbox_pools sp
  ON sp.id = seed.sandbox_pool_id
JOIN vms v
  ON v.id = seed.vm_id
JOIN hosts h
  ON h.id = v.host_id
WHERE sp.vm_template_id = $1
  AND sp.fork
  AND seed.seed
  AND seed.status = 'READY'
  AND v.status = 'RUNNING'
  AND h.status = 'UP'
ORDER BY 2, seed.created_at, seed.id
        "#,
        vm_template_id
    )
    .fetch_all(pool)
    .await
}

/// The oldest ready member of the template's pool, never its seed, locked for
/// the caller's transaction to hand to a sandbox.
pub async fn claim_ready_for_template_tx(
    tx: &mut Transaction<'_, Postgres>,
    vm_template_id: Uuid,
) -> Result<Option<SandboxPoolMember>, sqlx::Error> {
    sqlx::query_as!(
        SandboxPoolMember,
        r#"
SELECT spm.id,
       spm.sandbox_pool_id,
       spm.vm_id,
       spm.status AS "status: _",
       spm.error_message,
       spm.created_at,
       spm.updated_at
FROM sandbox_pool_members spm
JOIN sandbox_pools sp
  ON sp.id = spm.sandbox_pool_id
WHERE sp.vm_template_id = $1
  AND spm.status = 'READY'
  AND NOT spm.seed
ORDER BY spm.created_at
FOR UPDATE OF spm SKIP LOCKED
LIMIT 1
        "#,
        vm_template_id
    )
    .fetch_optional(tx.as_mut())
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{hosts::HostStatus, vms},
        test_support::{TestDatabase, resolved_vm},
    };

    async fn running_vm(db: &TestDatabase, name: &str, host_id: Uuid) -> Uuid {
        let mut tx = db.pool.begin().await.unwrap();
        let vm_id = vms::create_tx_with_status(
            &mut tx,
            &resolved_vm(name),
            Some(host_id),
            VmStatus::Running,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        vm_id
    }

    async fn add_member(db: &TestDatabase, pool_id: Uuid, vm_id: Uuid, seed: bool, status: &str) {
        sqlx::query(
            "INSERT INTO sandbox_pool_members (sandbox_pool_id, vm_id, seed, status) \
             VALUES ($1, $2, $3, $4::sandbox_pool_member_status)",
        )
        .bind(pool_id)
        .bind(vm_id)
        .bind(seed)
        .bind(status)
        .execute(&db.pool)
        .await
        .unwrap();
    }

    /// A template and its pool, forking or not.
    async fn add_pool(db: &TestDatabase, name: &str, fork: bool) -> (Uuid, Uuid) {
        let template_id: Uuid =
            sqlx::query_scalar("INSERT INTO vm_templates (name) VALUES ($1) RETURNING id")
                .bind(name)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        let pool_id: Uuid = sqlx::query_scalar(
            "INSERT INTO sandbox_pools (vm_template_id, min_ready, fork, seed_hosts) \
             VALUES ($1, 1, $2, 4) RETURNING id",
        )
        .bind(template_id)
        .bind(fork)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        (template_id, pool_id)
    }

    #[tokio::test]
    async fn the_seed_whose_host_holds_the_fewest_members_forks_first() {
        let db = TestDatabase::new().await;
        let busy = db.add_host("busy", HostStatus::Up).await;
        let idle = db.add_host("idle", HostStatus::Up).await;
        let booting = db.add_host("booting", HostStatus::Up).await;
        let away = db.add_host("away", HostStatus::Maintenance).await;
        let (template_id, pool_id) = add_pool(&db, "forking", true).await;

        let busy_seed = running_vm(&db, "busy-seed", busy).await;
        add_member(&db, pool_id, busy_seed, true, "READY").await;
        // Members booting or ready count; one being destroyed doesn't.
        for (name, status) in [
            ("busy-ready", "READY"),
            ("busy-provisioning", "PROVISIONING"),
            ("busy-destroying", "DESTROYING"),
        ] {
            let vm_id = running_vm(&db, name, busy).await;
            add_member(&db, pool_id, vm_id, false, status).await;
        }
        // Another pool's member doesn't count either.
        let (_, other_pool) = add_pool(&db, "other", false).await;
        let other = running_vm(&db, "other-ready", idle).await;
        add_member(&db, other_pool, other, false, "READY").await;
        let idle_seed = running_vm(&db, "idle-seed", idle).await;
        add_member(&db, pool_id, idle_seed, true, "READY").await;
        // A seed still booting, or on a host that isn't up, doesn't fork.
        let booting_seed = running_vm(&db, "booting-seed", booting).await;
        add_member(&db, pool_id, booting_seed, true, "PROVISIONING").await;
        let away_seed = running_vm(&db, "away-seed", away).await;
        add_member(&db, pool_id, away_seed, true, "READY").await;

        let seeds: Vec<(Uuid, i64)> = forkable_seeds(&db.pool, template_id)
            .await
            .unwrap()
            .into_iter()
            .map(|seed| (seed.vm_id, seed.members))
            .collect();
        assert_eq!(seeds, [(idle_seed, 0), (busy_seed, 2)]);

        // A pool that doesn't fork has none to fork.
        sqlx::query("UPDATE sandbox_pools SET fork = false WHERE id = $1")
            .bind(pool_id)
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(
            forkable_seeds(&db.pool, template_id)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
