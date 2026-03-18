use serde::{Deserialize, Serialize};
use sqlx::{PgPool, types::Json};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::model::{
    network_interfaces, vm_disks,
    vms::{self, BootMode, Hypervisor, NewVmNetwork},
};

fn empty_json_object() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct VmTemplate {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub hypervisor: Option<Hypervisor>,
    pub boot_vcpus: Option<i32>,
    pub max_vcpus: Option<i32>,
    pub cpu_topology: Option<serde_json::Value>,
    pub kvm_hyperv: Option<bool>,
    pub memory_size: Option<i64>,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: Option<bool>,
    pub memory_shared: Option<bool>,
    pub memory_hugepages: Option<bool>,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: Option<bool>,
    pub memory_thp: Option<bool>,
    pub boot_source_id: Option<Uuid>,
    pub root_disk_object_id: Option<Uuid>,
    pub boot_mode: Option<BootMode>,
    pub image_ref: Option<String>,
    pub cloud_init_user_data: Option<String>,
    pub cloud_init_meta_data: Option<String>,
    pub cloud_init_network_config: Option<String>,
    pub network_id: Option<Uuid>,
    pub networks: Option<Vec<NewVmNetwork>>,
    pub config: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct VmTemplateRow {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub hypervisor: Option<Hypervisor>,
    pub boot_vcpus: Option<i32>,
    pub max_vcpus: Option<i32>,
    pub cpu_topology: Option<Json<serde_json::Value>>,
    pub kvm_hyperv: Option<bool>,
    pub memory_size: Option<i64>,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: Option<bool>,
    pub memory_shared: Option<bool>,
    pub memory_hugepages: Option<bool>,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: Option<bool>,
    pub memory_thp: Option<bool>,
    pub boot_source_id: Option<Uuid>,
    pub root_disk_object_id: Option<Uuid>,
    pub boot_mode: Option<BootMode>,
    pub image_ref: Option<String>,
    pub cloud_init_user_data: Option<String>,
    pub cloud_init_meta_data: Option<String>,
    pub cloud_init_network_config: Option<String>,
    pub network_id: Option<Uuid>,
    pub networks: Option<Json<Vec<NewVmNetwork>>>,
    pub config: Json<serde_json::Value>,
}

impl From<VmTemplateRow> for VmTemplate {
    fn from(row: VmTemplateRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            hypervisor: row.hypervisor,
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
            boot_source_id: row.boot_source_id,
            root_disk_object_id: row.root_disk_object_id,
            boot_mode: row.boot_mode,
            image_ref: row.image_ref,
            cloud_init_user_data: row.cloud_init_user_data,
            cloud_init_meta_data: row.cloud_init_meta_data,
            cloud_init_network_config: row.cloud_init_network_config,
            network_id: row.network_id,
            networks: row.networks.map(|value| value.0),
            config: row.config.0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewVmTemplate {
    pub name: String,
    pub description: Option<String>,
    pub hypervisor: Option<Hypervisor>,
    pub boot_vcpus: Option<i32>,
    pub max_vcpus: Option<i32>,
    pub cpu_topology: Option<serde_json::Value>,
    pub kvm_hyperv: Option<bool>,
    pub memory_size: Option<i64>,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: Option<bool>,
    pub memory_shared: Option<bool>,
    pub memory_hugepages: Option<bool>,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: Option<bool>,
    pub memory_thp: Option<bool>,
    pub boot_source_id: Option<Uuid>,
    pub root_disk_object_id: Option<Uuid>,
    pub boot_mode: Option<BootMode>,
    pub image_ref: Option<String>,
    pub cloud_init_user_data: Option<String>,
    pub cloud_init_meta_data: Option<String>,
    pub cloud_init_network_config: Option<String>,
    pub network_id: Option<Uuid>,
    pub networks: Option<Vec<NewVmNetwork>>,
    #[serde(default = "empty_json_object")]
    pub config: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct CreateVmTemplateFromVmRequest {
    pub name: String,
    pub description: Option<String>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<VmTemplate>, sqlx::Error> {
    let rows = sqlx::query_as::<_, VmTemplateRow>(
        r#"
SELECT id,
       name,
       description,
       hypervisor,
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
       boot_source_id,
       root_disk_object_id,
       boot_mode,
       image_ref,
       cloud_init_user_data,
       cloud_init_meta_data,
       cloud_init_network_config,
       network_id,
       networks,
       config
FROM vm_templates
ORDER BY name
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &PgPool, vm_template_id: Uuid) -> Result<VmTemplate, sqlx::Error> {
    let row = sqlx::query_as::<_, VmTemplateRow>(
        r#"
SELECT id,
       name,
       description,
       hypervisor,
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
       boot_source_id,
       root_disk_object_id,
       boot_mode,
       image_ref,
       cloud_init_user_data,
       cloud_init_meta_data,
       cloud_init_network_config,
       network_id,
       networks,
       config
FROM vm_templates
WHERE id = $1
        "#,
    )
    .bind(vm_template_id)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn get_by_name(pool: &PgPool, name: &str) -> Result<VmTemplate, sqlx::Error> {
    let row = sqlx::query_as::<_, VmTemplateRow>(
        r#"
SELECT id,
       name,
       description,
       hypervisor,
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
       boot_source_id,
       root_disk_object_id,
       boot_mode,
       image_ref,
       cloud_init_user_data,
       cloud_init_meta_data,
       cloud_init_network_config,
       network_id,
       networks,
       config
FROM vm_templates
WHERE name = $1
        "#,
    )
    .bind(name)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn create(pool: &PgPool, new_vm_template: NewVmTemplate) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let cpu_topology = new_vm_template.cpu_topology.map(Json);
    let networks = new_vm_template.networks.map(Json);
    let config = Json(new_vm_template.config);

    sqlx::query(
        r#"
INSERT INTO vm_templates (
    id,
    name,
    description,
    hypervisor,
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
    boot_source_id,
    root_disk_object_id,
    boot_mode,
    image_ref,
    cloud_init_user_data,
    cloud_init_meta_data,
    cloud_init_network_config,
    network_id,
    networks,
    config
)
VALUES (
    $1, $2, $3, $4, $5, $6, $7, $8,
    $9, $10, $11, $12, $13, $14, $15, $16,
    $17, $18, $19, $20, $21, $22, $23, $24, $25,
    $26
)
        "#,
    )
    .bind(id)
    .bind(new_vm_template.name)
    .bind(new_vm_template.description)
    .bind(new_vm_template.hypervisor)
    .bind(new_vm_template.boot_vcpus)
    .bind(new_vm_template.max_vcpus)
    .bind(cpu_topology)
    .bind(new_vm_template.kvm_hyperv)
    .bind(new_vm_template.memory_size)
    .bind(new_vm_template.memory_hotplug_size)
    .bind(new_vm_template.memory_mergeable)
    .bind(new_vm_template.memory_shared)
    .bind(new_vm_template.memory_hugepages)
    .bind(new_vm_template.memory_hugepage_size)
    .bind(new_vm_template.memory_prefault)
    .bind(new_vm_template.memory_thp)
    .bind(new_vm_template.boot_source_id)
    .bind(new_vm_template.root_disk_object_id)
    .bind(new_vm_template.boot_mode)
    .bind(new_vm_template.image_ref)
    .bind(new_vm_template.cloud_init_user_data)
    .bind(new_vm_template.cloud_init_meta_data)
    .bind(new_vm_template.cloud_init_network_config)
    .bind(new_vm_template.network_id)
    .bind(networks)
    .bind(config)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn create_from_vm(
    pool: &PgPool,
    vm_id: Uuid,
    request: CreateVmTemplateFromVmRequest,
) -> Result<Uuid, sqlx::Error> {
    let vm = vms::get(pool, vm_id).await?;
    let interfaces = network_interfaces::list_by_vm(pool, vm_id).await?;
    let disks = vm_disks::list_by_vm(pool, vm_id).await?;
    let inferred_network_id = match interfaces.as_slice() {
        [interface] => interface.network_id,
        _ => None,
    };
    let root_disk_object_id = disks
        .iter()
        .find(|disk| disk.boot_order == Some(0))
        .or_else(|| disks.first())
        .and_then(|disk| disk.storage_object_id);

    let template = NewVmTemplate {
        name: request.name,
        description: request.description.or(vm.description.clone()),
        hypervisor: Some(vm.hypervisor),
        boot_vcpus: Some(vm.boot_vcpus),
        max_vcpus: Some(vm.max_vcpus),
        cpu_topology: vm.cpu_topology.clone(),
        kvm_hyperv: Some(vm.kvm_hyperv),
        memory_size: Some(vm.memory_size),
        memory_hotplug_size: vm.memory_hotplug_size,
        memory_mergeable: Some(vm.memory_mergeable),
        memory_shared: Some(vm.memory_shared),
        memory_hugepages: Some(vm.memory_hugepages),
        memory_hugepage_size: vm.memory_hugepage_size,
        memory_prefault: Some(vm.memory_prefault),
        memory_thp: Some(vm.memory_thp),
        boot_source_id: vm.boot_source_id,
        root_disk_object_id,
        boot_mode: Some(vm.boot_mode),
        image_ref: vm.image_ref.clone(),
        cloud_init_user_data: vm.cloud_init_user_data.clone(),
        cloud_init_meta_data: vm.cloud_init_meta_data.clone(),
        cloud_init_network_config: vm.cloud_init_network_config.clone(),
        network_id: inferred_network_id,
        networks: None,
        config: vm.config.clone(),
    };

    create(pool, template).await
}

pub async fn delete(pool: &PgPool, vm_template_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
DELETE FROM vm_templates
WHERE id = $1
        "#,
    )
    .bind(vm_template_id)
    .execute(pool)
    .await?;

    Ok(())
}
