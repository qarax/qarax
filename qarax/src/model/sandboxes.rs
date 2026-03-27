use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type};
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

#[derive(sqlx::FromRow, Debug, Clone)]
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

pub async fn create(
    pool: &PgPool,
    id: Uuid,
    vm_id: Uuid,
    vm_template_id: Option<Uuid>,
    name: &str,
    idle_timeout_secs: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
INSERT INTO sandboxes (id, vm_id, vm_template_id, name, idle_timeout_secs)
VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(id)
    .bind(vm_id)
    .bind(vm_template_id)
    .bind(name)
    .bind(idle_timeout_secs)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &PgPool, sandbox_id: Uuid) -> Result<SandboxRow, Error> {
    let row = sqlx::query_as::<_, SandboxRow>(
        r#"
SELECT id,
       vm_id,
       vm_template_id,
       name,
       status,
       idle_timeout_secs,
       last_activity_at,
       error_message,
       created_at
FROM sandboxes
WHERE id = $1
        "#,
    )
    .bind(sandbox_id)
    .fetch_optional(pool)
    .await
    .map_err(Error::Sqlx)?
    .ok_or(Error::NotFound)?;
    Ok(row)
}

pub async fn get_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<Option<SandboxRow>, sqlx::Error> {
    sqlx::query_as::<_, SandboxRow>(
        r#"
SELECT id,
       vm_id,
       vm_template_id,
       name,
       status,
       idle_timeout_secs,
       last_activity_at,
       error_message,
       created_at
FROM sandboxes
WHERE vm_id = $1
        "#,
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await
}

pub async fn list(pool: &PgPool) -> Result<Vec<SandboxRow>, sqlx::Error> {
    sqlx::query_as::<_, SandboxRow>(
        r#"
SELECT id,
       vm_id,
       vm_template_id,
       name,
       status,
       idle_timeout_secs,
       last_activity_at,
       error_message,
       created_at
FROM sandboxes
ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
}

pub async fn update_status(
    pool: &PgPool,
    sandbox_id: Uuid,
    status: SandboxStatus,
    error_message: Option<String>,
) -> Result<(), sqlx::Error> {
    sqlx::query(r#"UPDATE sandboxes SET status = $1, error_message = $2 WHERE id = $3"#)
        .bind(status)
        .bind(error_message)
        .bind(sandbox_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn touch_activity(pool: &PgPool, sandbox_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(r#"UPDATE sandboxes SET last_activity_at = NOW() WHERE id = $1"#)
        .bind(sandbox_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_expired(pool: &PgPool) -> Result<Vec<SandboxRow>, sqlx::Error> {
    sqlx::query_as::<_, SandboxRow>(
        r#"
SELECT id,
       vm_id,
       vm_template_id,
       name,
       status,
       idle_timeout_secs,
       last_activity_at,
       error_message,
       created_at
FROM sandboxes
WHERE status = 'READY'
  AND last_activity_at + (idle_timeout_secs || ' seconds')::interval < NOW()
        "#,
    )
    .fetch_all(pool)
    .await
}

pub async fn delete(pool: &PgPool, sandbox_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(r#"DELETE FROM sandboxes WHERE id = $1"#)
        .bind(sandbox_id)
        .execute(pool)
        .await?;
    Ok(())
}
