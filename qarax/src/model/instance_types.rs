use serde::{Deserialize, Serialize};
use sqlx::{PgPool, types::Json};
use utoipa::ToSchema;
use uuid::Uuid;

fn empty_json_object() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct InstanceType {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub architecture: Option<String>,
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub cpu_topology: Option<serde_json::Value>,
    pub kvm_hyperv: Option<bool>,
    pub memory_size: i64,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: Option<bool>,
    pub memory_shared: Option<bool>,
    pub memory_hugepages: Option<bool>,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: Option<bool>,
    pub memory_thp: Option<bool>,
    pub accelerator_config: serde_json::Value,
    pub numa_config: Option<serde_json::Value>,
}

#[derive(sqlx::FromRow)]
struct InstanceTypeRow {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub architecture: Option<String>,
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub cpu_topology: Option<Json<serde_json::Value>>,
    pub kvm_hyperv: Option<bool>,
    pub memory_size: i64,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: Option<bool>,
    pub memory_shared: Option<bool>,
    pub memory_hugepages: Option<bool>,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: Option<bool>,
    pub memory_thp: Option<bool>,
    pub accelerator_config: Json<serde_json::Value>,
    pub numa_config: Option<Json<serde_json::Value>>,
}

impl From<InstanceTypeRow> for InstanceType {
    fn from(row: InstanceTypeRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            architecture: row.architecture,
            boot_vcpus: row.boot_vcpus,
            max_vcpus: row.max_vcpus,
            cpu_topology: row.cpu_topology.map(|value| value.0),
            kvm_hyperv: row.kvm_hyperv,
            memory_size: row.memory_size,
            memory_hotplug_size: row.memory_hotplug_size,
            memory_mergeable: row.memory_mergeable,
            memory_shared: row.memory_shared,
            memory_hugepages: row.memory_hugepages,
            memory_hugepage_size: row.memory_hugepage_size,
            memory_prefault: row.memory_prefault,
            memory_thp: row.memory_thp,
            accelerator_config: row.accelerator_config.0,
            numa_config: row.numa_config.map(|v| v.0),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewInstanceType {
    pub name: String,
    pub description: Option<String>,
    pub architecture: Option<String>,
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub cpu_topology: Option<serde_json::Value>,
    pub kvm_hyperv: Option<bool>,
    pub memory_size: i64,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: Option<bool>,
    pub memory_shared: Option<bool>,
    pub memory_hugepages: Option<bool>,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: Option<bool>,
    pub memory_thp: Option<bool>,
    #[serde(default = "empty_json_object")]
    pub accelerator_config: serde_json::Value,
    pub numa_config: Option<serde_json::Value>,
}

pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
) -> Result<Vec<InstanceType>, sqlx::Error> {
    let rows = sqlx::query_as::<_, InstanceTypeRow>(
        r#"
SELECT id,
       name,
       description,
       architecture,
       boot_vcpus,
       max_vcpus,
       cpu_topology,
       kvm_hyperv,
       memory_size,
       memory_hotplug_size,
       memory_mergeable,
       memory_shared,
       memory_hugepages,
       memory_hugepage_size,
       memory_prefault,
       memory_thp,
       accelerator_config,
       numa_config
FROM instance_types
WHERE ($1::text IS NULL OR name = $1)
ORDER BY name
        "#,
    )
    .bind(name_filter)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &PgPool, instance_type_id: Uuid) -> Result<InstanceType, sqlx::Error> {
    let row = sqlx::query_as::<_, InstanceTypeRow>(
        r#"
SELECT id,
       name,
       description,
       architecture,
       boot_vcpus,
       max_vcpus,
       cpu_topology,
       kvm_hyperv,
       memory_size,
       memory_hotplug_size,
       memory_mergeable,
       memory_shared,
       memory_hugepages,
       memory_hugepage_size,
       memory_prefault,
       memory_thp,
       accelerator_config,
       numa_config
FROM instance_types
WHERE id = $1
        "#,
    )
    .bind(instance_type_id)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn get_by_name(pool: &PgPool, name: &str) -> Result<InstanceType, sqlx::Error> {
    let row = sqlx::query_as::<_, InstanceTypeRow>(
        r#"
SELECT id,
       name,
       description,
       architecture,
       boot_vcpus,
       max_vcpus,
       cpu_topology,
       kvm_hyperv,
       memory_size,
       memory_hotplug_size,
       memory_mergeable,
       memory_shared,
       memory_hugepages,
       memory_hugepage_size,
       memory_prefault,
       memory_thp,
       accelerator_config,
       numa_config
FROM instance_types
WHERE name = $1
        "#,
    )
    .bind(name)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn create(
    pool: &PgPool,
    new_instance_type: NewInstanceType,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let cpu_topology = new_instance_type.cpu_topology.map(Json);
    let accelerator_config = Json(new_instance_type.accelerator_config);
    let numa_config = new_instance_type.numa_config.map(Json);
    let architecture = new_instance_type
        .architecture
        .as_deref()
        .and_then(common::architecture::normalize_architecture);

    sqlx::query(
        r#"
INSERT INTO instance_types (
    id,
    name,
    description,
    architecture,
    boot_vcpus,
    max_vcpus,
    cpu_topology,
    kvm_hyperv,
    memory_size,
    memory_hotplug_size,
    memory_mergeable,
    memory_shared,
    memory_hugepages,
    memory_hugepage_size,
    memory_prefault,
    memory_thp,
    accelerator_config,
    numa_config
)
VALUES (
    $1, $2, $3, $4, $5, $6, $7, $8,
    $9, $10, $11, $12, $13, $14, $15, $16, $17, $18
)
        "#,
    )
    .bind(id)
    .bind(new_instance_type.name)
    .bind(new_instance_type.description)
    .bind(architecture)
    .bind(new_instance_type.boot_vcpus)
    .bind(new_instance_type.max_vcpus)
    .bind(cpu_topology)
    .bind(new_instance_type.kvm_hyperv)
    .bind(new_instance_type.memory_size)
    .bind(new_instance_type.memory_hotplug_size)
    .bind(new_instance_type.memory_mergeable)
    .bind(new_instance_type.memory_shared)
    .bind(new_instance_type.memory_hugepages)
    .bind(new_instance_type.memory_hugepage_size)
    .bind(new_instance_type.memory_prefault)
    .bind(new_instance_type.memory_thp)
    .bind(accelerator_config)
    .bind(numa_config)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, instance_type_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
DELETE FROM instance_types
WHERE id = $1
        "#,
    )
    .bind(instance_type_id)
    .execute(pool)
    .await?;

    Ok(())
}
