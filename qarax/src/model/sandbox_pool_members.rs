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

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct SandboxPoolMember {
    pub id: Uuid,
    pub sandbox_pool_id: Uuid,
    pub vm_id: Uuid,
    pub status: SandboxPoolMemberStatus,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn create(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
    vm_id: Uuid,
) -> Result<SandboxPoolMember, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
INSERT INTO sandbox_pool_members (id, sandbox_pool_id, vm_id)
VALUES ($1, $2, $3)
        "#,
    )
    .bind(id)
    .bind(sandbox_pool_id)
    .bind(vm_id)
    .execute(pool)
    .await?;

    get(pool, id).await
}

pub async fn get(pool: &PgPool, member_id: Uuid) -> Result<SandboxPoolMember, sqlx::Error> {
    sqlx::query_as::<_, SandboxPoolMember>(
        r#"
SELECT id, sandbox_pool_id, vm_id, status, error_message, created_at, updated_at
FROM sandbox_pool_members
WHERE id = $1
        "#,
    )
    .bind(member_id)
    .fetch_one(pool)
    .await
}

pub async fn list_ready_by_pool(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
) -> Result<Vec<SandboxPoolMember>, sqlx::Error> {
    sqlx::query_as::<_, SandboxPoolMember>(
        r#"
SELECT id, sandbox_pool_id, vm_id, status, error_message, created_at, updated_at
FROM sandbox_pool_members
WHERE sandbox_pool_id = $1
  AND status = 'READY'
ORDER BY created_at
        "#,
    )
    .bind(sandbox_pool_id)
    .fetch_all(pool)
    .await
}

pub async fn list_error_by_pool(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
) -> Result<Vec<SandboxPoolMember>, sqlx::Error> {
    sqlx::query_as::<_, SandboxPoolMember>(
        r#"
SELECT id, sandbox_pool_id, vm_id, status, error_message, created_at, updated_at
FROM sandbox_pool_members
WHERE sandbox_pool_id = $1
  AND status = 'ERROR'
ORDER BY created_at
        "#,
    )
    .bind(sandbox_pool_id)
    .fetch_all(pool)
    .await
}

pub async fn list_by_pool(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
) -> Result<Vec<SandboxPoolMember>, sqlx::Error> {
    sqlx::query_as::<_, SandboxPoolMember>(
        r#"
SELECT id, sandbox_pool_id, vm_id, status, error_message, created_at, updated_at
FROM sandbox_pool_members
WHERE sandbox_pool_id = $1
ORDER BY created_at
        "#,
    )
    .bind(sandbox_pool_id)
    .fetch_all(pool)
    .await
}

pub async fn count_by_status(
    pool: &PgPool,
    sandbox_pool_id: Uuid,
    status: SandboxPoolMemberStatus,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query_scalar::<_, i64>(
        r#"
SELECT COUNT(*)::bigint
FROM sandbox_pool_members
WHERE sandbox_pool_id = $1
  AND status = $2
        "#,
    )
    .bind(sandbox_pool_id)
    .bind(status)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn update_status(
    pool: &PgPool,
    member_id: Uuid,
    status: SandboxPoolMemberStatus,
    error_message: Option<String>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE sandbox_pool_members
SET status = $2,
    error_message = $3,
    updated_at = NOW()
WHERE id = $1
        "#,
    )
    .bind(member_id)
    .bind(status)
    .bind(error_message)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &PgPool, member_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sandbox_pool_members WHERE id = $1")
        .bind(member_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_tx(
    tx: &mut Transaction<'_, Postgres>,
    member_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sandbox_pool_members WHERE id = $1")
        .bind(member_id)
        .execute(tx.as_mut())
        .await?;
    Ok(())
}

pub async fn claim_ready_for_template_tx(
    tx: &mut Transaction<'_, Postgres>,
    vm_template_id: Uuid,
) -> Result<Option<SandboxPoolMember>, sqlx::Error> {
    sqlx::query_as::<_, SandboxPoolMember>(
        r#"
SELECT spm.id,
       spm.sandbox_pool_id,
       spm.vm_id,
       spm.status,
       spm.error_message,
       spm.created_at,
       spm.updated_at
FROM sandbox_pool_members spm
JOIN sandbox_pools sp
  ON sp.id = spm.sandbox_pool_id
WHERE sp.vm_template_id = $1
  AND spm.status = 'READY'
ORDER BY spm.created_at
FOR UPDATE OF spm SKIP LOCKED
LIMIT 1
        "#,
    )
    .bind(vm_template_id)
    .fetch_optional(tx.as_mut())
    .await
}
