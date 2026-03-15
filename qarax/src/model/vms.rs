use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type, types::Json};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::model::network_interfaces::{InterfaceType, RateLimiterConfig, VhostMode};

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct CpuTopology {
    pub threads_per_core: Option<i32>,
    pub cores_per_die: Option<i32>,
    pub dies_per_package: Option<i32>,
    pub packages: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Vm {
    pub id: Uuid,
    pub name: String,
    pub host_id: Option<Uuid>,
    pub status: VmStatus,
    pub hypervisor: Hypervisor,
    pub boot_source_id: Option<Uuid>,
    pub boot_mode: BootMode,
    pub description: Option<String>,

    // CPU configuration
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub cpu_topology: Option<serde_json::Value>,
    pub kvm_hyperv: bool,

    // Memory configuration (in bytes)
    pub memory_size: i64,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: bool,
    pub memory_shared: bool,
    pub memory_hugepages: bool,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: bool,
    pub memory_thp: bool,

    // OCI image boot support
    pub image_ref: Option<String>,

    // Legacy config field for flexibility
    pub config: serde_json::Value,
}

#[derive(sqlx::FromRow)]
pub struct VmRow {
    pub id: Uuid,
    pub name: String,
    pub host_id: Option<Uuid>,
    pub status: VmStatus,
    pub hypervisor: Hypervisor,
    pub boot_source_id: Option<Uuid>,
    pub boot_mode: BootMode,
    pub description: Option<String>,

    // CPU configuration
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub cpu_topology: Option<Json<serde_json::Value>>,
    pub kvm_hyperv: bool,

    // Memory configuration
    pub memory_size: i64,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: bool,
    pub memory_shared: bool,
    pub memory_hugepages: bool,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: bool,
    pub memory_thp: bool,

    pub image_ref: Option<String>,

    pub config: Json<serde_json::Value>,
}

impl From<VmRow> for Vm {
    fn from(row: VmRow) -> Self {
        Vm {
            id: row.id,
            name: row.name,
            status: row.status,
            host_id: row.host_id,
            hypervisor: row.hypervisor,
            boot_source_id: row.boot_source_id,
            boot_mode: row.boot_mode,
            description: row.description,

            boot_vcpus: row.boot_vcpus,
            max_vcpus: row.max_vcpus,
            cpu_topology: row.cpu_topology.map(|t| t.0),
            kvm_hyperv: row.kvm_hyperv,

            memory_size: row.memory_size,
            memory_hotplug_size: row.memory_hotplug_size,
            memory_mergeable: row.memory_mergeable,
            memory_shared: row.memory_shared,
            memory_hugepages: row.memory_hugepages,
            memory_hugepage_size: row.memory_hugepage_size,
            memory_prefault: row.memory_prefault,
            memory_thp: row.memory_thp,

            image_ref: row.image_ref,

            config: row.config.0,
        }
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "hypervisor")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Hypervisor {
    CloudHv,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "boot_mode")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum BootMode {
    Kernel,
    Firmware,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "vm_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VmStatus {
    Unknown,
    Pending,
    Created,
    Running,
    Paused,
    Shutdown,
    Migrating,
}

/// Network interface config for create-VM request. Passed to qarax-node; id is required.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewVmNetwork {
    /// Unique device id (e.g. "net0")
    pub id: String,
    /// Network ID for managed networking (IPAM)
    #[serde(default)]
    pub network_id: Option<Uuid>,
    /// Guest MAC address (optional)
    pub mac: Option<String>,
    /// Pre-created TAP device name (optional)
    pub tap: Option<String>,
    /// IPv4 or IPv6 address (optional)
    pub ip: Option<String>,
    /// Network mask (optional)
    pub mask: Option<String>,
    /// MTU (optional)
    pub mtu: Option<i32>,

    // Advanced fields
    /// Host-side MAC address
    pub host_mac: Option<String>,
    /// Override interface type (inferred from tap/vhost_user if not set)
    pub interface_type: Option<InterfaceType>,
    /// Enable vhost-user networking
    pub vhost_user: Option<bool>,
    /// Unix socket path for vhost-user backend
    pub vhost_socket: Option<String>,
    /// Vhost-user mode (client or server)
    pub vhost_mode: Option<VhostMode>,
    /// Number of virtio queues
    pub num_queues: Option<i32>,
    /// Size of each queue
    pub queue_size: Option<i32>,
    /// Rate limiter configuration
    pub rate_limiter: Option<RateLimiterConfig>,
    /// Enable TCP Segmentation Offload
    pub offload_tso: Option<bool>,
    /// Enable UDP Fragmentation Offload
    pub offload_ufo: Option<bool>,
    /// Enable checksum offload
    pub offload_csum: Option<bool>,
    /// PCI segment number
    pub pci_segment: Option<i32>,
    /// Enable IOMMU for the device
    pub iommu: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewVm {
    pub name: String,
    pub hypervisor: Hypervisor,

    // CPU
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub cpu_topology: Option<serde_json::Value>,
    pub kvm_hyperv: Option<bool>,

    // Memory
    pub memory_size: i64,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: Option<bool>,
    pub memory_shared: Option<bool>,
    pub memory_hugepages: Option<bool>,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: Option<bool>,
    pub memory_thp: Option<bool>,

    pub boot_source_id: Option<Uuid>,
    pub boot_mode: Option<BootMode>,
    pub description: Option<String>,

    /// OCI image reference to use as root filesystem (e.g. "docker.io/library/ubuntu:22.04").
    /// When set, the handler will check whether the selected host has an OverlayBD storage pool.
    /// If so, the image is served via lazy block loading (virtio-blk); otherwise via virtiofs.
    pub image_ref: Option<String>,

    /// Network ID to attach the VM to (triggers IPAM allocation).
    pub network_id: Option<Uuid>,

    /// Optional network interfaces to attach at create time (passed to qarax-node).
    #[serde(default)]
    pub networks: Option<Vec<NewVmNetwork>>,

    #[serde(default)]
    pub config: serde_json::Value,
}

pub async fn list(pool: &PgPool) -> Result<Vec<Vm>, sqlx::Error> {
    let vms: Vec<VmRow> = sqlx::query_as!(
        VmRow,
        r#"
SELECT id,
        name,
        status as "status: _",
        host_id as "host_id?",
        hypervisor as "hypervisor: _",
        boot_source_id as "boot_source_id?",
        boot_mode as "boot_mode: _",
        description as "description?",
        boot_vcpus,
        max_vcpus,
        cpu_topology as "cpu_topology: _",
        kvm_hyperv as "kvm_hyperv!",
        memory_size,
        memory_hotplug_size as "memory_hotplug_size?",
        memory_mergeable as "memory_mergeable!",
        memory_shared as "memory_shared!",
        memory_hugepages as "memory_hugepages!",
        memory_hugepage_size as "memory_hugepage_size?",
        memory_prefault as "memory_prefault!",
        memory_thp as "memory_thp!",
        image_ref as "image_ref?",
        config as "config: _"
FROM vms
        "#
    )
    .fetch_all(pool)
    .await?;

    let vms: Vec<Vm> = vms.into_iter().map(|vm: VmRow| vm.into()).collect();
    Ok(vms)
}

pub async fn get(pool: &PgPool, vm_id: Uuid) -> Result<Vm, sqlx::Error> {
    let vm: VmRow = sqlx::query_as!(
        VmRow,
        r#"
SELECT id,
        name,
        status as "status: _",
        host_id as "host_id?",
        hypervisor as "hypervisor: _",
        boot_source_id as "boot_source_id?",
        boot_mode as "boot_mode: _",
        description as "description?",
        boot_vcpus,
        max_vcpus,
        cpu_topology as "cpu_topology: _",
        kvm_hyperv as "kvm_hyperv!",
        memory_size,
        memory_hotplug_size as "memory_hotplug_size?",
        memory_mergeable as "memory_mergeable!",
        memory_shared as "memory_shared!",
        memory_hugepages as "memory_hugepages!",
        memory_hugepage_size as "memory_hugepage_size?",
        memory_prefault as "memory_prefault!",
        memory_thp as "memory_thp!",
        image_ref as "image_ref?",
        config as "config: _"
FROM vms
WHERE id = $1
        "#,
        vm_id
    )
    .fetch_one(pool)
    .await?;

    Ok(vm.into())
}

pub async fn create(pool: &PgPool, vm: &NewVm) -> Result<Uuid, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let id = create_tx(&mut tx, vm).await?;
    tx.commit().await?;
    Ok(id)
}

/// Creates a VM row inside the given transaction. Used by the handler to roll back on node failure.
pub async fn create_tx(
    tx: &mut Transaction<'_, Postgres>,
    vm: &NewVm,
) -> Result<Uuid, sqlx::Error> {
    create_tx_with_status(tx, vm, VmStatus::Created).await
}

/// Creates a VM row with a specific initial status inside the given transaction.
pub async fn create_tx_with_status(
    tx: &mut Transaction<'_, Postgres>,
    vm: &NewVm,
    status: VmStatus,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let cpu_topology = vm.cpu_topology.as_ref().map(|t| Json(t.clone()));
    let config = Json(&vm.config);

    sqlx::query(
        r#"
INSERT INTO vms (
    id, name, status, hypervisor,
    boot_vcpus, max_vcpus, cpu_topology, kvm_hyperv,
    memory_size, memory_hotplug_size, memory_mergeable, memory_shared,
    memory_hugepages, memory_hugepage_size, memory_prefault, memory_thp,
    boot_source_id, boot_mode, description, image_ref, config
)
VALUES (
    $1, $2, $3, $4,
    $5, $6, $7, $8,
    $9, $10, $11, $12,
    $13, $14, $15, $16,
    $17, $18, $19, $20, $21
)
        "#,
    )
    .bind(id)
    .bind(&vm.name)
    .bind(status)
    .bind(vm.hypervisor.clone())
    .bind(vm.boot_vcpus)
    .bind(vm.max_vcpus)
    .bind(cpu_topology)
    .bind(vm.kvm_hyperv.unwrap_or(false))
    .bind(vm.memory_size)
    .bind(vm.memory_hotplug_size)
    .bind(vm.memory_mergeable.unwrap_or(false))
    .bind(vm.memory_shared.unwrap_or(false))
    .bind(vm.memory_hugepages.unwrap_or(false))
    .bind(vm.memory_hugepage_size)
    .bind(vm.memory_prefault.unwrap_or(false))
    .bind(vm.memory_thp.unwrap_or(false))
    .bind(vm.boot_source_id)
    .bind(vm.boot_mode.clone().unwrap_or(BootMode::Kernel))
    .bind(&vm.description)
    .bind(&vm.image_ref)
    .bind(config)
    .execute(tx.as_mut())
    .await?;

    Ok(id)
}

pub async fn list_active(pool: &PgPool) -> Result<Vec<Vm>, sqlx::Error> {
    let vms = sqlx::query_as::<_, VmRow>(
        r#"
SELECT id,
        name,
        status,
        host_id,
        hypervisor,
        boot_source_id,
        boot_mode,
        description,
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
        image_ref,
        config
FROM vms
WHERE status NOT IN ('SHUTDOWN', 'UNKNOWN', 'PENDING')
  AND host_id IS NOT NULL
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(vms.into_iter().map(|vm| vm.into()).collect())
}

pub async fn update_status(
    pool: &PgPool,
    vm_id: Uuid,
    status: VmStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE vms SET status = $1 WHERE id = $2")
        .bind(status)
        .bind(vm_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn update_host_id(pool: &PgPool, vm_id: Uuid, host_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE vms SET host_id = $1 WHERE id = $2")
        .bind(host_id)
        .bind(vm_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn delete(pool: &PgPool, vm_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM vms WHERE id = $1")
        .bind(vm_id)
        .execute(pool)
        .await?;

    Ok(())
}
