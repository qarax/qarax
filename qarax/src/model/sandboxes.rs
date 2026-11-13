use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::errors::Error;
use crate::model::vms::VmStatus;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Sandbox {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub vm_template_id: Option<Uuid>,
    pub name: String,
    pub status: SandboxStatus,
    pub idle_timeout_secs: i32,
    pub last_activity_at: DateTime<Utc>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    // Enriched fields joined from vms table
    pub ip_address: Option<String>,
    pub vm_status: Option<VmStatus>,
}

#[derive(Debug, Clone)]
pub struct SandboxRow {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub vm_template_id: Option<Uuid>,
    pub name: String,
    pub status: SandboxStatus,
    pub idle_timeout_secs: i32,
    pub last_activity_at: DateTime<Utc>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<SandboxRow> for Sandbox {
    fn from(row: SandboxRow) -> Self {
        Sandbox {
            id: row.id,
            vm_id: row.vm_id,
            vm_template_id: row.vm_template_id,
            name: row.name,
            status: row.status,
            idle_timeout_secs: row.idle_timeout_secs,
            last_activity_at: row.last_activity_at,
            error_message: row.error_message,
            created_at: row.created_at,
            ip_address: None,
            vm_status: None,
        }
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "sandbox_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SandboxStatus {
    Provisioning,
    Ready,
    Error,
    Destroying,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewSandbox {
    pub name: String,
    pub vm_template_id: Uuid,
    pub idle_timeout_secs: Option<i32>,
    pub instance_type_id: Option<Uuid>,
    pub network_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct CreateSandboxResponse {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub job_id: Uuid,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct ExecSandboxRequest {
    pub command: Vec<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct ExecSandboxResponse {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

pub async fn create(
    pool: &PgPool,
    id: Uuid,
    vm_id: Uuid,
    vm_template_id: Option<Uuid>,
    name: &str,
    idle_timeout_secs: i32,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    create_tx_with_status(
        &mut tx,
        id,
        vm_id,
        vm_template_id,
        name,
        idle_timeout_secs,
        SandboxStatus::Provisioning,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn create_tx_with_status(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    vm_id: Uuid,
    vm_template_id: Option<Uuid>,
    name: &str,
    idle_timeout_secs: i32,
    status: SandboxStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
INSERT INTO sandboxes (id, vm_id, vm_template_id, name, status, idle_timeout_secs)
VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id,
        vm_id,
        vm_template_id,
        name,
        status as SandboxStatus,
        idle_timeout_secs
    )
    .execute(tx.as_mut())
    .await?;
    Ok(())
}

pub async fn get(pool: &PgPool, sandbox_id: Uuid) -> Result<SandboxRow, Error> {
    let row = sqlx::query_as!(
        SandboxRow,
        r#"
SELECT id,
       vm_id,
       vm_template_id,
       name,
       status AS "status: _",
       idle_timeout_secs,
       last_activity_at,
       error_message,
       created_at
FROM sandboxes
WHERE id = $1
        "#,
        sandbox_id
    )
    .fetch_optional(pool)
    .await
    .map_err(Error::Sqlx)?
    .ok_or(Error::NotFound)?;
    Ok(row)
}

pub async fn get_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<Option<SandboxRow>, sqlx::Error> {
    sqlx::query_as!(
        SandboxRow,
        r#"
SELECT id,
       vm_id,
       vm_template_id,
       name,
       status AS "status: _",
       idle_timeout_secs,
       last_activity_at,
       error_message,
       created_at
FROM sandboxes
WHERE vm_id = $1
        "#,
        vm_id
    )
    .fetch_optional(pool)
    .await
}

#[derive(Debug, Clone)]
pub struct SandboxWithDetailsRow {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub vm_template_id: Option<Uuid>,
    pub name: String,
    pub status: SandboxStatus,
    pub idle_timeout_secs: i32,
    pub last_activity_at: DateTime<Utc>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub vm_status: Option<VmStatus>,
    pub ip_address: Option<String>,
}

impl From<SandboxWithDetailsRow> for Sandbox {
    fn from(row: SandboxWithDetailsRow) -> Self {
        Sandbox {
            id: row.id,
            vm_id: row.vm_id,
            vm_template_id: row.vm_template_id,
            name: row.name,
            status: row.status,
            idle_timeout_secs: row.idle_timeout_secs,
            last_activity_at: row.last_activity_at,
            error_message: row.error_message,
            created_at: row.created_at,
            ip_address: row.ip_address,
            vm_status: row.vm_status,
        }
    }
}

pub async fn list_with_details(pool: &PgPool) -> Result<Vec<SandboxWithDetailsRow>, sqlx::Error> {
    sqlx::query_as!(
        SandboxWithDetailsRow,
        r#"
SELECT s.id,
       s.vm_id,
       s.vm_template_id,
       s.name,
       s.status AS "status: _",
       s.idle_timeout_secs,
       s.last_activity_at,
       s.error_message,
       s.created_at,
       v.status AS "vm_status: _",
       ni.ip_address::text AS ip_address
FROM sandboxes s
LEFT JOIN vms v ON v.id = s.vm_id
LEFT JOIN LATERAL (
    SELECT ip_address
    FROM network_interfaces
    WHERE vm_id = s.vm_id AND ip_address IS NOT NULL
    LIMIT 1
) ni ON TRUE
ORDER BY s.created_at DESC
        "#
    )
    .fetch_all(pool)
    .await
}

pub async fn get_with_details(
    pool: &PgPool,
    sandbox_id: Uuid,
) -> Result<SandboxWithDetailsRow, Error> {
    sqlx::query_as!(
        SandboxWithDetailsRow,
        r#"
SELECT s.id,
       s.vm_id,
       s.vm_template_id,
       s.name,
       s.status AS "status: _",
       s.idle_timeout_secs,
       s.last_activity_at,
       s.error_message,
       s.created_at,
       v.status AS "vm_status: _",
       ni.ip_address::text AS ip_address
FROM sandboxes s
LEFT JOIN vms v ON v.id = s.vm_id
LEFT JOIN LATERAL (
    SELECT ip_address
    FROM network_interfaces
    WHERE vm_id = s.vm_id AND ip_address IS NOT NULL
    LIMIT 1
) ni ON TRUE
WHERE s.id = $1
        "#,
        sandbox_id
    )
    .fetch_optional(pool)
    .await
    .map_err(Error::Sqlx)?
    .ok_or(Error::NotFound)
}

pub async fn list(pool: &PgPool) -> Result<Vec<SandboxRow>, sqlx::Error> {
    sqlx::query_as!(
        SandboxRow,
        r#"
SELECT id,
       vm_id,
       vm_template_id,
       name,
       status AS "status: _",
       idle_timeout_secs,
       last_activity_at,
       error_message,
       created_at
FROM sandboxes
ORDER BY created_at DESC
        "#
    )
    .fetch_all(pool)
    .await
}

// A sandbox moves PROVISIONING -> READY | ERROR, and from any status to
// DESTROYING. `error_message` says why it failed: set with ERROR and kept
// while the sandbox is destroyed. Each write below moves it only from the
// statuses that may precede its own and returns whether it did.

/// PROVISIONING -> READY.
pub async fn mark_ready(pool: &PgPool, sandbox_id: Uuid) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE sandboxes SET status = 'READY' WHERE id = $1 AND status = 'PROVISIONING'",
        sandbox_id
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

/// PROVISIONING -> ERROR.
pub async fn mark_error(
    pool: &PgPool,
    sandbox_id: Uuid,
    error_message: &str,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE sandboxes SET status = 'ERROR', error_message = $2 \
         WHERE id = $1 AND status = 'PROVISIONING'",
        sandbox_id,
        error_message
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

/// Any status but DESTROYING -> DESTROYING. `false` means someone else is
/// already destroying it.
pub async fn mark_destroying(pool: &PgPool, sandbox_id: Uuid) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE sandboxes SET status = 'DESTROYING' WHERE id = $1 AND status <> 'DESTROYING'",
        sandbox_id
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

pub async fn touch_activity(pool: &PgPool, sandbox_id: Uuid) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"UPDATE sandboxes SET last_activity_at = NOW() WHERE id = $1"#,
        sandbox_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn list_expired(pool: &PgPool) -> Result<Vec<SandboxRow>, sqlx::Error> {
    sqlx::query_as!(
        SandboxRow,
        r#"
SELECT id,
       vm_id,
       vm_template_id,
       name,
       status AS "status: _",
       idle_timeout_secs,
       last_activity_at,
       error_message,
       created_at
FROM sandboxes
WHERE status = 'READY'
  AND last_activity_at + (idle_timeout_secs || ' seconds')::interval < NOW()
        "#
    )
    .fetch_all(pool)
    .await
}

pub async fn delete(pool: &PgPool, sandbox_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(r#"DELETE FROM sandboxes WHERE id = $1"#, sandbox_id)
        .execute(pool)
        .await?;
    Ok(())
}
