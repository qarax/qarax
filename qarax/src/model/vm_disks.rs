use serde::{Deserialize, Serialize};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VmDisk {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub storage_object_id: Option<Uuid>, // Optional for vhost-user
    pub disk_id: String,                 // Unique identifier for Cloud Hypervisor
    pub device_path: String,             // Device path in guest
    pub boot_order: Option<i32>,
    pub read_only: bool,

    // Direct I/O
    pub direct: bool,

    // vhost-user configuration
    pub vhost_user: bool,
    pub vhost_socket: Option<String>,

    // Performance
    pub num_queues: i32,
    pub queue_size: i32,
    pub rate_limiter: Option<serde_json::Value>,
    pub rate_limit_group: Option<String>,

    // PCI configuration
    pub pci_segment: i32,
    pub serial_number: Option<String>,

    // Legacy config
    pub config: serde_json::Value,
}

#[derive(sqlx::FromRow)]
pub struct VmDiskRow {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub storage_object_id: Option<Uuid>,
    pub disk_id: String,
    pub device_path: String,
    pub boot_order: Option<i32>,
    pub read_only: bool,
    pub direct: bool,
    pub vhost_user: bool,
    pub vhost_socket: Option<String>,
    pub num_queues: i32,
    pub queue_size: i32,
    pub rate_limiter: Option<Json<serde_json::Value>>,
    pub rate_limit_group: Option<String>,
    pub pci_segment: i32,
    pub serial_number: Option<String>,
    pub config: Json<serde_json::Value>,
}

impl From<VmDiskRow> for VmDisk {
    fn from(row: VmDiskRow) -> Self {
        VmDisk {
            id: row.id,
            vm_id: row.vm_id,
            storage_object_id: row.storage_object_id,
            disk_id: row.disk_id,
            device_path: row.device_path,
            boot_order: row.boot_order,
            read_only: row.read_only,
            direct: row.direct,
            vhost_user: row.vhost_user,
            vhost_socket: row.vhost_socket,
            num_queues: row.num_queues,
            queue_size: row.queue_size,
            rate_limiter: row.rate_limiter.map(|r| r.0),
            rate_limit_group: row.rate_limit_group,
            pci_segment: row.pci_segment,
            serial_number: row.serial_number,
            config: row.config.0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewVmDisk {
    pub vm_id: Uuid,
    pub storage_object_id: Option<Uuid>,
    pub disk_id: String,
    pub device_path: String,
    pub boot_order: Option<i32>,
    pub read_only: Option<bool>,
    pub direct: Option<bool>,
    pub vhost_user: Option<bool>,
    pub vhost_socket: Option<String>,
    pub num_queues: Option<i32>,
    pub queue_size: Option<i32>,
    pub rate_limiter: Option<serde_json::Value>,
    pub rate_limit_group: Option<String>,
    pub pci_segment: Option<i32>,
    pub serial_number: Option<String>,

    #[serde(default)]
    pub config: serde_json::Value,
}

pub async fn list(pool: &PgPool) -> Result<Vec<VmDisk>, sqlx::Error> {
    let vm_disks: Vec<VmDiskRow> = sqlx::query_as!(
        VmDiskRow,
        r#"
SELECT id,
        vm_id,
        storage_object_id as "storage_object_id?",
        disk_id,
        device_path,
        boot_order as "boot_order?",
        read_only as "read_only!",
        direct as "direct!",
        vhost_user as "vhost_user!",
        vhost_socket as "vhost_socket?",
        num_queues as "num_queues!",
        queue_size as "queue_size!",
        rate_limiter as "rate_limiter: _",
        rate_limit_group as "rate_limit_group?",
        pci_segment as "pci_segment!",
        serial_number as "serial_number?",
        config as "config: _"
FROM vm_disks
        "#
    )
    .fetch_all(pool)
    .await?;

    let vm_disks: Vec<VmDisk> = vm_disks
        .into_iter()
        .map(|vd: VmDiskRow| vd.into())
        .collect();
    Ok(vm_disks)
}

pub async fn get(pool: &PgPool, disk_id: Uuid) -> Result<VmDisk, sqlx::Error> {
    let vm_disk: VmDiskRow = sqlx::query_as!(
        VmDiskRow,
        r#"
SELECT id,
        vm_id,
        storage_object_id as "storage_object_id?",
        disk_id,
        device_path,
        boot_order as "boot_order?",
        read_only as "read_only!",
        direct as "direct!",
        vhost_user as "vhost_user!",
        vhost_socket as "vhost_socket?",
        num_queues as "num_queues!",
        queue_size as "queue_size!",
        rate_limiter as "rate_limiter: _",
        rate_limit_group as "rate_limit_group?",
        pci_segment as "pci_segment!",
        serial_number as "serial_number?",
        config as "config: _"
FROM vm_disks
WHERE id = $1
        "#,
        disk_id
    )
    .fetch_one(pool)
    .await?;

    Ok(vm_disk.into())
}

pub async fn list_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<Vec<VmDisk>, sqlx::Error> {
    let vm_disks: Vec<VmDiskRow> = sqlx::query_as!(
        VmDiskRow,
        r#"
SELECT id,
        vm_id,
        storage_object_id as "storage_object_id?",
        disk_id,
        device_path,
        boot_order as "boot_order?",
        read_only as "read_only!",
        direct as "direct!",
        vhost_user as "vhost_user!",
        vhost_socket as "vhost_socket?",
        num_queues as "num_queues!",
        queue_size as "queue_size!",
        rate_limiter as "rate_limiter: _",
        rate_limit_group as "rate_limit_group?",
        pci_segment as "pci_segment!",
        serial_number as "serial_number?",
        config as "config: _"
FROM vm_disks
WHERE vm_id = $1
ORDER BY boot_order NULLS LAST, device_path
        "#,
        vm_id
    )
    .fetch_all(pool)
    .await?;

    let vm_disks: Vec<VmDisk> = vm_disks
        .into_iter()
        .map(|vd: VmDiskRow| vd.into())
        .collect();
    Ok(vm_disks)
}
