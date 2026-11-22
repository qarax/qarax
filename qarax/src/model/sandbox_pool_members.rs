use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

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

/// A PROVISIONING member of the pool: its seed, which is never claimed, or a
/// member a sandbox claims.
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

/// The pool's seed, if it has one.
pub async fn seed(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
) -> Result<Option<SandboxPoolMember>, sqlx::Error> {
    sqlx::query_as!(
        SandboxPoolMember,
        r#"
SELECT id, sandbox_pool_id, vm_id, status AS "status: _", error_message, created_at, updated_at
FROM sandbox_pool_members
WHERE sandbox_pool_id = $1 AND seed
        "#,
        sandbox_pool_id
    )
    .fetch_optional(pool)
    .await
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

/// The VM of the template's pool's seed, when the pool forks and its seed is
/// ready and runs: a sandbox is forked from it when the pool has no member
/// ready.
pub async fn running_seed_for_template(
    pool: &PgPool,
    vm_template_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
SELECT seed.vm_id
FROM sandbox_pool_members seed
JOIN sandbox_pools sp
  ON sp.id = seed.sandbox_pool_id
JOIN vms v
  ON v.id = seed.vm_id
WHERE sp.vm_template_id = $1
  AND sp.fork
  AND seed.seed
  AND seed.status = 'READY'
  AND v.status = 'RUNNING'
        "#,
        vm_template_id
    )
    .fetch_optional(pool)
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
