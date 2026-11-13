use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, types::Json};
use utoipa::ToSchema;
use uuid::Uuid;

type PgTransaction<'a> = Transaction<'a, Postgres>;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct VmDisk {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub storage_object_id: Option<Uuid>, // Optional for vhost-user
    pub logical_name: String,            // Device name for Cloud Hypervisor (e.g. "vda", "rootfs")
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

    /// For OverlayBD disks: ID of the OverlaybdUpper StorageObject holding the
    /// persistent upper layer (upper.data + upper.index) on a Local or NFS pool.
    /// None = ephemeral (deleted on VM stop); Some = persistent across VM delete.
    pub upper_storage_object_id: Option<Uuid>,
}

pub struct VmDiskRow {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub storage_object_id: Option<Uuid>,
    pub logical_name: String,
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
    pub upper_storage_object_id: Option<Uuid>,
}

impl From<VmDiskRow> for VmDisk {
    fn from(row: VmDiskRow) -> Self {
        VmDisk {
            id: row.id,
            vm_id: row.vm_id,
            storage_object_id: row.storage_object_id,
            logical_name: row.logical_name,
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
            upper_storage_object_id: row.upper_storage_object_id,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct NewVmDisk {
    pub vm_id: Uuid,
    pub storage_object_id: Option<Uuid>,
    pub logical_name: String,
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

    pub upper_storage_object_id: Option<Uuid>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<VmDisk>, sqlx::Error> {
    let vm_disks: Vec<VmDiskRow> = sqlx::query_as!(
        VmDiskRow,
        r#"
SELECT id,
        vm_id,
        storage_object_id as "storage_object_id?",
        logical_name,
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
        config as "config: _",
        upper_storage_object_id as "upper_storage_object_id?"
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

pub async fn get(pool: &PgPool, id: Uuid) -> Result<VmDisk, sqlx::Error> {
    let vm_disk: VmDiskRow = sqlx::query_as!(
        VmDiskRow,
        r#"
SELECT id,
        vm_id,
        storage_object_id as "storage_object_id?",
        logical_name,
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
        config as "config: _",
        upper_storage_object_id as "upper_storage_object_id?"
FROM vm_disks
WHERE id = $1
        "#,
        id
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
        logical_name,
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
        config as "config: _",
        upper_storage_object_id as "upper_storage_object_id?"
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

pub async fn list_by_storage_object(
    pool: &PgPool,
    storage_object_id: Uuid,
) -> Result<Vec<VmDisk>, sqlx::Error> {
    let vm_disks: Vec<VmDiskRow> = sqlx::query_as!(
        VmDiskRow,
        r#"
SELECT id,
        vm_id,
        storage_object_id,
        logical_name,
        device_path,
        boot_order,
        read_only AS "read_only!",
        direct AS "direct!",
        vhost_user AS "vhost_user!",
        vhost_socket,
        num_queues AS "num_queues!",
        queue_size AS "queue_size!",
        rate_limiter AS "rate_limiter: _",
        rate_limit_group,
        pci_segment AS "pci_segment!",
        serial_number,
        config AS "config: _",
        upper_storage_object_id
FROM vm_disks
WHERE storage_object_id = $1
ORDER BY vm_id, boot_order NULLS LAST, device_path
        "#,
        storage_object_id
    )
    .fetch_all(pool)
    .await?;

    Ok(vm_disks.into_iter().map(Into::into).collect())
}

pub async fn list_by_storage_object_tx(
    tx: &mut PgTransaction<'_>,
    storage_object_id: Uuid,
) -> Result<Vec<VmDisk>, sqlx::Error> {
    let vm_disks: Vec<VmDiskRow> = sqlx::query_as!(
        VmDiskRow,
        r#"
SELECT id,
       vm_id,
       storage_object_id,
       logical_name,
       device_path,
       boot_order,
       read_only AS "read_only!",
       direct AS "direct!",
       vhost_user AS "vhost_user!",
       vhost_socket,
       num_queues AS "num_queues!",
       queue_size AS "queue_size!",
       rate_limiter AS "rate_limiter: _",
       rate_limit_group,
       pci_segment AS "pci_segment!",
       serial_number,
       config AS "config: _",
       upper_storage_object_id
FROM vm_disks
WHERE storage_object_id = $1
ORDER BY vm_id, boot_order NULLS LAST, device_path
        "#,
        storage_object_id
    )
    .fetch_all(tx.as_mut())
    .await?;

    Ok(vm_disks.into_iter().map(Into::into).collect())
}

pub async fn create(pool: &PgPool, disk: &NewVmDisk) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    create_with_id(pool, id, disk).await?;

    Ok(id)
}

pub async fn create_tx(tx: &mut PgTransaction<'_>, disk: &NewVmDisk) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    create_tx_with_id(tx, id, disk).await?;

    Ok(id)
}

async fn create_with_id(pool: &PgPool, id: Uuid, disk: &NewVmDisk) -> Result<(), sqlx::Error> {
    insert_disk_query(id, disk).execute(pool).await?;

    Ok(())
}

async fn create_tx_with_id(
    tx: &mut PgTransaction<'_>,
    id: Uuid,
    disk: &NewVmDisk,
) -> Result<(), sqlx::Error> {
    insert_disk_query(id, disk).execute(tx.as_mut()).await?;

    Ok(())
}

fn insert_disk_query<'a>(
    id: Uuid,
    disk: &'a NewVmDisk,
) -> sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments> {
    sqlx::query!(
        r#"
INSERT INTO vm_disks (
    id, vm_id, storage_object_id, logical_name, device_path, boot_order,
    read_only, direct, vhost_user, vhost_socket,
    num_queues, queue_size, rate_limiter, rate_limit_group,
    pci_segment, serial_number, config, upper_storage_object_id
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        "#,
        id,
        disk.vm_id,
        disk.storage_object_id,
        &disk.logical_name,
        &disk.device_path,
        disk.boot_order,
        disk.read_only.unwrap_or(false),
        disk.direct.unwrap_or(false),
        disk.vhost_user.unwrap_or(false),
        disk.vhost_socket.as_deref(),
        disk.num_queues.unwrap_or(1),
        disk.queue_size.unwrap_or(128),
        disk.rate_limiter.as_ref(),
        disk.rate_limit_group.as_deref(),
        disk.pci_segment.unwrap_or(0),
        disk.serial_number.as_deref(),
        &disk.config,
        disk.upper_storage_object_id
    )
}

pub async fn delete(pool: &PgPool, disk_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM vm_disks WHERE id = $1", disk_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Replace the storage object and upper layer references on a disk record.
/// Used by vm commit to swap the OCI image disk for a raw disk.
pub async fn update_storage_object(
    pool: &PgPool,
    disk_id: Uuid,
    storage_object_id: Uuid,
    upper_storage_object_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE vm_disks SET storage_object_id = $1, upper_storage_object_id = $2 WHERE id = $3",
        storage_object_id,
        upper_storage_object_id,
        disk_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Row type for the OCI disk join query (find_oci_disk_for_vm).
pub struct OciDiskRow {
    pub disk_id: Uuid,
    pub oci_storage_object_id: Uuid,
    pub oci_config: sqlx::types::Json<serde_json::Value>,
    pub upper_storage_object_id: Option<Uuid>,
    pub upper_config: Option<sqlx::types::Json<serde_json::Value>>,
}

/// Find the OCI image disk on a VM in a single SQL join.
/// Joins vm_disks → storage_objects → storage_pools, filtering for
/// OCI_IMAGE objects on OVERLAYBD pools. Also left-joins the upper
/// storage object to get its config in the same query.
pub async fn find_oci_disk_for_vm(
    pool: &PgPool,
    vm_id: Uuid,
) -> Result<Option<OciDiskRow>, sqlx::Error> {
    let row = sqlx::query_as!(
        OciDiskRow,
        r#"
        SELECT
            vd.id                       AS disk_id,
            so.id                       AS oci_storage_object_id,
            so.config AS "oci_config: _",
            vd.upper_storage_object_id,
            upper_so.config AS "upper_config: _"
        FROM vm_disks vd
        JOIN storage_objects so ON vd.storage_object_id = so.id
        JOIN storage_pools sp ON so.storage_pool_id = sp.id
        LEFT JOIN storage_objects upper_so ON vd.upper_storage_object_id = upper_so.id
        WHERE vd.vm_id = $1
          AND so.object_type = 'OCI_IMAGE'
          AND sp.pool_type = 'OVERLAYBD'
        LIMIT 1
        "#,
        vm_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_by_logical_name(
    pool: &PgPool,
    vm_id: Uuid,
    logical_name: &str,
) -> Result<Option<VmDisk>, sqlx::Error> {
    let row: Option<VmDiskRow> = sqlx::query_as!(
        VmDiskRow,
        r#"
SELECT id,
        vm_id,
        storage_object_id as "storage_object_id?",
        logical_name,
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
        config as "config: _",
        upper_storage_object_id as "upper_storage_object_id?"
FROM vm_disks
WHERE vm_id = $1 AND logical_name = $2
        "#,
        vm_id,
        logical_name
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.into()))
}
