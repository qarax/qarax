use super::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Kernel {
    pub id: Uuid,
    pub name: String,
    pub storage_id: Uuid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewKernel {
    pub name: String,
    pub storage_id: Uuid,
}

#[derive(Error, Debug)]
pub enum KernelError {
    #[error("Failed to list kernels: {0}")]
    List(#[from] sqlx::Error),

    #[error("Failed to add kernel: {0}, error: {1}")]
    Add(String, sqlx::Error),

    #[error("Can't find kernel: {0}, error: {1}")]
    Find(Uuid, sqlx::Error),
}

pub async fn list(pool: &PgPool) -> Result<Vec<Kernel>, KernelError> {
    let kernels = sqlx::query_as!(
        Kernel,
        r#"
SELECT id, name, storage_id
FROM kernels
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(KernelError::List)?;

    Ok(kernels)
}

pub async fn by_id(pool: &PgPool, kernel_id: &Uuid) -> Result<Kernel, KernelError> {
    let kernel = sqlx::query_as!(
        Kernel,
        r#"
SELECT id, name, storage_id
FROM kernels
WHERE id = $1
        "#,
        kernel_id
    )
    .fetch_one(pool)
    .await
    .map_err(|e| KernelError::Find(*kernel_id, e))?;

    Ok(kernel)
}

pub async fn add(pool: &PgPool, kernel: &NewKernel) -> Result<Uuid, KernelError> {
    let rec = sqlx::query!(
        r#"
INSERT INTO kernels (name, storage_id)
VALUES ( $1, $2)
RETURNING id
        "#,
        kernel.name,
        kernel.storage_id
    )
    .fetch_one(pool)
    .await
    .map_err(|e| KernelError::Add(kernel.name.to_owned(), e))?;

    Ok(rec.id)
}
