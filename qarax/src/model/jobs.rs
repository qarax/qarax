use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, sqlx::FromRow)]
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
    VmStart,
    VmMigrate,
    DiskCreate,
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

pub struct NewJob {
    pub job_type: JobType,
    pub description: Option<String>,
    pub resource_id: Option<Uuid>,
    pub resource_type: Option<String>,
}

pub async fn create(pool: &PgPool, new_job: NewJob) -> Result<Job, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query(
        r#"
INSERT INTO jobs (id, job_type, description, resource_id, resource_type)
VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(id)
    .bind(&new_job.job_type)
    .bind(&new_job.description)
    .bind(new_job.resource_id)
    .bind(&new_job.resource_type)
    .execute(pool)
    .await?;

    get(pool, id).await
}

pub async fn get(pool: &PgPool, job_id: Uuid) -> Result<Job, sqlx::Error> {
    let job = sqlx::query_as::<_, Job>(
        r#"
SELECT id,
       job_type,
       status,
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
    )
    .bind(job_id)
    .fetch_one(pool)
    .await?;

    Ok(job)
}

pub async fn mark_running(pool: &PgPool, job_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE jobs
SET status = 'RUNNING',
    started_at = NOW(),
    updated_at = NOW()
WHERE id = $1
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_completed(
    pool: &PgPool,
    job_id: Uuid,
    result: Option<serde_json::Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE jobs
SET status = 'COMPLETED',
    result = $2,
    progress = 100,
    completed_at = NOW(),
    updated_at = NOW()
WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(result.map(sqlx::types::Json))
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_failed(pool: &PgPool, job_id: Uuid, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE jobs
SET status = 'FAILED',
    error = $2,
    completed_at = NOW(),
    updated_at = NOW()
WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(error)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_progress(
    pool: &PgPool,
    job_id: Uuid,
    progress: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE jobs
SET progress = $2,
    updated_at = NOW()
WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(progress)
    .execute(pool)
    .await?;

    Ok(())
}
