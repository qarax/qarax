use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VmRng {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub src: String,
    pub iommu: bool,
}

pub async fn get_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<Option<VmRng>, sqlx::Error> {
    let rng = sqlx::query_as!(
        VmRng,
        r#"
SELECT id,
        vm_id,
        src,
        iommu as "iommu!"
FROM vm_rng
WHERE vm_id = $1
        "#,
        vm_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(rng)
}

pub async fn get(pool: &PgPool, rng_id: Uuid) -> Result<VmRng, sqlx::Error> {
    let rng = sqlx::query_as!(
        VmRng,
        r#"
SELECT id,
        vm_id,
        src,
        iommu as "iommu!"
FROM vm_rng
WHERE id = $1
        "#,
        rng_id
    )
    .fetch_one(pool)
    .await?;

    Ok(rng)
}
