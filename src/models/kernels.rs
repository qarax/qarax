use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use crate::handlers::ServerError;

use super::{ValidName, ValidationError};

#[derive(Error, Debug)]
pub enum KernelError {
    #[error("Inavlid name: {0}")]
    InvalidName(#[from] ValidationError),
    #[error("Unexpected failure: {0}")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<sqlx::Error> for KernelError {
    fn from(e: sqlx::Error) -> Self {
        KernelError::Other(Box::new(e))
    }
}

impl From<KernelError> for ServerError {
    fn from(e: KernelError) -> Self {
        match e {
            KernelError::InvalidName(e) => ServerError::Validation(e.to_string()),
            KernelError::Other(e) => ServerError::Internal(e.to_string()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewKernel {
    pub name: ValidName,
    pub url: Option<String>,
    pub storage_id: Uuid,
    pub volume_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Kernel {
    id: Uuid,
    volume_id: Uuid,
}

pub async fn add(pool: &PgPool, kernel: &NewKernel) -> Result<Uuid, KernelError> {
    let rec = sqlx::query!(
        r#"
INSERT INTO kernels (volume_id)
VALUES ( $1)
RETURNING id
        "#,
        kernel.volume_id.unwrap(),
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn list(pool: &PgPool) -> Result<Vec<Kernel>, KernelError> {
    let kernels = sqlx::query_as!(
        Kernel,
        r#"
SELECT id, volume_id
FROM kernels
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(kernels)
}

pub async fn by_id(pool: &PgPool, kernel_id: Uuid) -> Result<Kernel, KernelError> {
    let kernel = sqlx::query_as!(
        Kernel,
        r#"
SELECT id, volume_id
FROM kernels
WHERE id = $1
        "#,
        kernel_id
    )
    .fetch_one(pool)
    .await?;

    Ok(kernel)
}
