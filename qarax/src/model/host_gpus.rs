use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, sqlx::FromRow)]
pub struct HostGpu {
    pub id: Uuid,
    pub host_id: Uuid,
    pub pci_address: String,
    pub model: Option<String>,
    pub vendor: Option<String>,
    pub vram_bytes: Option<i64>,
    pub iommu_group: i32,
    pub vm_id: Option<Uuid>,
    pub discovered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// GPU info from node discovery (mirrors proto GpuInfo)
pub struct GpuDiscovery {
    pub pci_address: String,
    pub model: String,
    pub vendor: String,
    pub vram_bytes: i64,
    pub iommu_group: i32,
}

/// Typed accelerator_config from instance types / VM requests
#[derive(Serialize, Deserialize, Debug, Clone, Default, ToSchema)]
pub struct AcceleratorConfig {
    pub gpu_count: i32,
    #[serde(default)]
    pub gpu_vendor: Option<String>,
    #[serde(default)]
    pub gpu_model: Option<String>,
    #[serde(default)]
    pub min_vram_bytes: Option<i64>,
}

impl AcceleratorConfig {
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        if value.is_null() || value.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            return None;
        }
        serde_json::from_value(value.clone()).ok()
    }
}

/// List all GPUs on a host.
pub async fn list_by_host(pool: &PgPool, host_id: Uuid) -> Result<Vec<HostGpu>, sqlx::Error> {
    sqlx::query_as::<_, HostGpu>(
        "SELECT id, host_id, pci_address, model, vendor, vram_bytes, iommu_group, vm_id, discovered_at, updated_at FROM host_gpus WHERE host_id = $1 ORDER BY pci_address",
    )
    .bind(host_id)
    .fetch_all(pool)
    .await
}

/// List all GPUs allocated to a VM.
pub async fn list_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<Vec<HostGpu>, sqlx::Error> {
    sqlx::query_as::<_, HostGpu>(
        "SELECT id, host_id, pci_address, model, vendor, vram_bytes, iommu_group, vm_id, discovered_at, updated_at FROM host_gpus WHERE vm_id = $1 ORDER BY pci_address",
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await
}

/// Sync GPU inventory from node discovery. Upserts discovered GPUs and removes stale entries.
pub async fn sync_gpus(
    pool: &PgPool,
    host_id: Uuid,
    gpus: &[GpuDiscovery],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    for gpu in gpus {
        sqlx::query(
            r#"
INSERT INTO host_gpus (host_id, pci_address, model, vendor, vram_bytes, iommu_group, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, NOW())
ON CONFLICT (host_id, pci_address)
DO UPDATE SET model = EXCLUDED.model,
              vendor = EXCLUDED.vendor,
              vram_bytes = EXCLUDED.vram_bytes,
              iommu_group = EXCLUDED.iommu_group,
              updated_at = NOW()
            "#,
        )
        .bind(host_id)
        .bind(&gpu.pci_address)
        .bind(&gpu.model)
        .bind(&gpu.vendor)
        .bind(gpu.vram_bytes)
        .bind(gpu.iommu_group)
        .execute(tx.as_mut())
        .await?;
    }

    // Remove GPUs that are no longer present on the host (stale entries)
    if gpus.is_empty() {
        sqlx::query("DELETE FROM host_gpus WHERE host_id = $1")
            .bind(host_id)
            .execute(tx.as_mut())
            .await?;
    } else {
        let pci_addresses: Vec<&str> = gpus.iter().map(|g| g.pci_address.as_str()).collect();
        sqlx::query("DELETE FROM host_gpus WHERE host_id = $1 AND pci_address != ALL($2)")
            .bind(host_id)
            .bind(&pci_addresses)
            .execute(tx.as_mut())
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Atomically allocate GPUs on a host to a VM. Uses FOR UPDATE SKIP LOCKED
/// to avoid contention with concurrent allocations.
pub async fn allocate_gpus(
    tx: &mut Transaction<'_, Postgres>,
    host_id: Uuid,
    vm_id: Uuid,
    count: i32,
    vendor: Option<&str>,
    model: Option<&str>,
    min_vram_bytes: Option<i64>,
) -> Result<Vec<HostGpu>, sqlx::Error> {
    let gpus = sqlx::query_as::<_, HostGpu>(
        r#"
UPDATE host_gpus
SET vm_id = $1, updated_at = NOW()
WHERE id IN (
    SELECT id FROM host_gpus
    WHERE host_id = $2
      AND vm_id IS NULL
      AND ($3::VARCHAR IS NULL OR vendor = $3)
      AND ($4::VARCHAR IS NULL OR model = $4)
      AND ($5::BIGINT IS NULL OR vram_bytes >= $5)
    ORDER BY pci_address
    FOR UPDATE SKIP LOCKED
    LIMIT $6
)
RETURNING id, host_id, pci_address, model, vendor, vram_bytes, iommu_group, vm_id, discovered_at, updated_at
        "#,
    )
    .bind(vm_id)
    .bind(host_id)
    .bind(vendor)
    .bind(model)
    .bind(min_vram_bytes)
    .bind(count)
    .fetch_all(tx.as_mut())
    .await?;

    Ok(gpus)
}

/// Release all GPUs allocated to a VM.
pub async fn deallocate_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<u64, sqlx::Error> {
    let result =
        sqlx::query("UPDATE host_gpus SET vm_id = NULL, updated_at = NOW() WHERE vm_id = $1")
            .bind(vm_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}
