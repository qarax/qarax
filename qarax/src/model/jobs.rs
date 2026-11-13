use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Job {
    pub id: Uuid,
    pub job_type: JobType,
    pub status: JobStatus,
    pub description: Option<String>,
    pub resource_id: Option<Uuid>,
    pub resource_type: Option<String>,
    pub progress: Option<i32>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "job_type")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum JobType {
    ImagePull,
    SandboxClaim,
    VmStart,
    VmMigrate,
    VmFailover,
    HostEvacuate,
    DiskCreate,
    VmCommit,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "job_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

/// Well-known resource type values for the `resource_type` column.
pub mod resource_types {
    pub const HOST: &str = "host";
    pub const SANDBOX: &str = "sandbox";
    pub const VM: &str = "vm";
}

pub struct NewJob {
    pub job_type: JobType,
    pub description: Option<String>,
    pub resource_id: Option<Uuid>,
    pub resource_type: Option<String>,
}

pub async fn create(pool: &PgPool, new_job: NewJob) -> Result<Job, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let job = create_tx(&mut tx, new_job).await?;
    tx.commit().await?;
    Ok(job)
}

pub async fn create_tx(
    tx: &mut Transaction<'_, Postgres>,
    new_job: NewJob,
) -> Result<Job, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query!(
        r#"
INSERT INTO jobs (id, job_type, description, resource_id, resource_type)
VALUES ($1, $2, $3, $4, $5)
        "#,
        id,
        &new_job.job_type as &JobType,
        new_job.description.as_deref(),
        new_job.resource_id,
        new_job.resource_type.as_deref()
    )
    .execute(tx.as_mut())
    .await?;

    get_tx(tx, id).await
}

pub async fn create_completed_tx(
    tx: &mut Transaction<'_, Postgres>,
    new_job: NewJob,
    result: Option<serde_json::Value>,
) -> Result<Job, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query!(
        r#"
INSERT INTO jobs (
    id,
    job_type,
    status,
    description,
    resource_id,
    resource_type,
    progress,
    result,
    started_at,
    completed_at
)
VALUES ($1, $2, 'COMPLETED', $3, $4, $5, 100, $6, NOW(), NOW())
        "#,
        id,
        &new_job.job_type as &JobType,
        new_job.description.as_deref(),
        new_job.resource_id,
        new_job.resource_type.as_deref(),
        result
    )
    .execute(tx.as_mut())
    .await?;

    get_tx(tx, id).await
}

pub async fn get(pool: &PgPool, job_id: Uuid) -> Result<Job, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let job = get_tx(&mut tx, job_id).await?;
    tx.commit().await?;
    Ok(job)
}

async fn get_tx(tx: &mut Transaction<'_, Postgres>, job_id: Uuid) -> Result<Job, sqlx::Error> {
    let job = sqlx::query_as!(
        Job,
        r#"
SELECT id,
       job_type AS "job_type: _",
       status AS "status: _",
       description,
       resource_id,
       resource_type,
       progress,
       result,
       error,
       created_at,
       updated_at,
       started_at,
       completed_at
FROM jobs
WHERE id = $1
        "#,
        job_id
    )
    .fetch_one(tx.as_mut())
    .await?;

    Ok(job)
}

// A job moves PENDING -> RUNNING -> COMPLETED | FAILED (or straight from
// PENDING to a final status). Each write below moves it only from the
// statuses that may precede its own, so a final status is never overwritten:
// the first completion or failure recorded wins.

/// Errors with `RowNotFound` unless the job is still pending.
pub async fn mark_running(pool: &PgPool, job_id: Uuid) -> Result<(), sqlx::Error> {
    let updated = sqlx::query!(
        r#"
UPDATE jobs
SET status = 'RUNNING',
    started_at = NOW(),
    updated_at = NOW()
WHERE id = $1 AND status = 'PENDING'
        "#,
        job_id
    )
    .execute(pool)
    .await?
    .rows_affected();

    if updated == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}

pub async fn mark_completed(
    pool: &PgPool,
    job_id: Uuid,
    result: Option<serde_json::Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
UPDATE jobs
SET status = 'COMPLETED',
    result = $2,
    progress = 100,
    completed_at = NOW(),
    updated_at = NOW()
WHERE id = $1 AND status IN ('PENDING', 'RUNNING')
        "#,
        job_id,
        result
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_failed(pool: &PgPool, job_id: Uuid, error: &str) -> Result<(), sqlx::Error> {
    mark_failed_with_result(pool, job_id, error, None).await
}

pub async fn mark_failed_with_result(
    pool: &PgPool,
    job_id: Uuid,
    error: &str,
    result: Option<serde_json::Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
UPDATE jobs
SET status = 'FAILED',
    error = $2,
    result = $3,
    completed_at = NOW(),
    updated_at = NOW()
WHERE id = $1 AND status IN ('PENDING', 'RUNNING')
        "#,
        job_id,
        error,
        result
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_progress(
    pool: &PgPool,
    job_id: Uuid,
    progress: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
UPDATE jobs
SET progress = $2,
    updated_at = NOW()
WHERE id = $1 AND status IN ('PENDING', 'RUNNING')
        "#,
        job_id,
        progress
    )
    .execute(pool)
    .await?;

    Ok(())
}
