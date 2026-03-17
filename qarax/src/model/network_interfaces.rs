use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type, types::Json};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::model::vms::NewVmNetwork;

pub type PgTransaction<'a> = Transaction<'a, Postgres>;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct RateLimiterConfig {
    pub bandwidth: Option<TokenBucket>,
    pub ops: Option<TokenBucket>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct TokenBucket {
    pub size: i64,
    pub refill_time: i64,
    pub one_time_burst: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NetworkInterface {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub network_id: Option<Uuid>,
    pub device_id: String,

    // Basic network config
    pub tap_name: Option<String>,
    pub mac_address: Option<String>,
    pub host_mac: Option<String>,
    pub ip_address: Option<String>,
    pub mtu: i32,

    // Interface type and mode
    pub interface_type: InterfaceType,
    pub vhost_user: bool,
    pub vhost_socket: Option<String>,
    pub vhost_mode: Option<String>,

    // Performance
    pub num_queues: i32,
    pub queue_size: i32,
    pub rate_limiter: Option<serde_json::Value>,

    // Offload features
    pub offload_tso: bool,
    pub offload_ufo: bool,
    pub offload_csum: bool,

    // PCI configuration
    pub pci_segment: i32,
    pub iommu: bool,
}

#[derive(sqlx::FromRow)]
pub struct NetworkInterfaceRow {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub network_id: Option<Uuid>,
    pub device_id: String,
    pub tap_name: Option<String>,
    pub mac_address: Option<String>,
    pub host_mac: Option<String>,
    pub ip_address: Option<String>,
    pub mtu: i32,
    pub interface_type: InterfaceType,
    pub vhost_user: bool,
    pub vhost_socket: Option<String>,
    pub vhost_mode: Option<String>,
    pub num_queues: i32,
    pub queue_size: i32,
    pub rate_limiter: Option<Json<serde_json::Value>>,
    pub offload_tso: bool,
    pub offload_ufo: bool,
    pub offload_csum: bool,
    pub pci_segment: i32,
    pub iommu: bool,
}

impl From<NetworkInterfaceRow> for NetworkInterface {
    fn from(row: NetworkInterfaceRow) -> Self {
        NetworkInterface {
            id: row.id,
            vm_id: row.vm_id,
            network_id: row.network_id,
            device_id: row.device_id,
            tap_name: row.tap_name,
            mac_address: row.mac_address,
            host_mac: row.host_mac,
            ip_address: row.ip_address,
            mtu: row.mtu,
            interface_type: row.interface_type,
            vhost_user: row.vhost_user,
            vhost_socket: row.vhost_socket,
            vhost_mode: row.vhost_mode,
            num_queues: row.num_queues,
            queue_size: row.queue_size,
            rate_limiter: row.rate_limiter.map(|r| r.0),
            offload_tso: row.offload_tso,
            offload_ufo: row.offload_ufo,
            offload_csum: row.offload_csum,
            pci_segment: row.pci_segment,
            iommu: row.iommu,
        }
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "snake_case")]
#[sqlx(type_name = "interface_type")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum InterfaceType {
    Macvtap,
    Tap,
    VhostUser,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VhostMode {
    Client,
    Server,
}

pub async fn list_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<Vec<NetworkInterface>, sqlx::Error> {
    let interfaces: Vec<NetworkInterfaceRow> = sqlx::query_as!(
        NetworkInterfaceRow,
        r#"
SELECT id,
        vm_id,
        network_id as "network_id?",
        device_id as "device_id!",
        tap_name as "tap_name?",
        mac_address::text as "mac_address?",
        host_mac::text as "host_mac?",
        ip_address::text as "ip_address?",
        mtu as "mtu!",
        interface_type as "interface_type: _",
        vhost_user as "vhost_user!",
        vhost_socket as "vhost_socket?",
        vhost_mode as "vhost_mode?",
        num_queues as "num_queues!",
        queue_size as "queue_size!",
        rate_limiter as "rate_limiter: _",
        offload_tso as "offload_tso!",
        offload_ufo as "offload_ufo!",
        offload_csum as "offload_csum!",
        pci_segment as "pci_segment!",
        iommu as "iommu!"
FROM network_interfaces
WHERE vm_id = $1
ORDER BY device_id
        "#,
        vm_id
    )
    .fetch_all(pool)
    .await?;

    Ok(interfaces.into_iter().map(|i| i.into()).collect())
}

pub async fn get(pool: &PgPool, interface_id: Uuid) -> Result<NetworkInterface, sqlx::Error> {
    let interface: NetworkInterfaceRow = sqlx::query_as!(
        NetworkInterfaceRow,
        r#"
SELECT id,
        vm_id,
        network_id as "network_id?",
        device_id as "device_id!",
        tap_name as "tap_name?",
        mac_address::text as "mac_address?",
        host_mac::text as "host_mac?",
        ip_address::text as "ip_address?",
        mtu as "mtu!",
        interface_type as "interface_type: _",
        vhost_user as "vhost_user!",
        vhost_socket as "vhost_socket?",
        vhost_mode as "vhost_mode?",
        num_queues as "num_queues!",
        queue_size as "queue_size!",
        rate_limiter as "rate_limiter: _",
        offload_tso as "offload_tso!",
        offload_ufo as "offload_ufo!",
        offload_csum as "offload_csum!",
        pci_segment as "pci_segment!",
        iommu as "iommu!"
FROM network_interfaces
WHERE id = $1
        "#,
        interface_id
    )
    .fetch_one(pool)
    .await?;

    Ok(interface.into())
}

pub async fn create(
    tx: &mut PgTransaction<'_>,
    vm_id: Uuid,
    n: &NewVmNetwork,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    // Infer interface_type from fields
    let interface_type = if n.vhost_user == Some(true) {
        InterfaceType::VhostUser
    } else if n.tap.is_some() {
        InterfaceType::Tap
    } else {
        InterfaceType::Macvtap
    };

    let vhost_mode = n.vhost_mode.as_ref().map(|m| m.to_string());
    let rate_limiter = n
        .rate_limiter
        .as_ref()
        .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null));

    sqlx::query(
        r#"
INSERT INTO network_interfaces (
    id, vm_id, network_id, device_id,
    tap_name, mac_address, host_mac, ip_address, mtu,
    interface_type, vhost_user, vhost_socket, vhost_mode,
    num_queues, queue_size, rate_limiter,
    offload_tso, offload_ufo, offload_csum,
    pci_segment, iommu
) VALUES (
    $1, $2, $3, $4,
    $5, $6::macaddr, $7::macaddr, $8::inet, $9,
    $10, $11, $12, $13,
    $14, $15, $16,
    $17, $18, $19,
    $20, $21
)
        "#,
    )
    .bind(id)
    .bind(vm_id)
    .bind(n.network_id) // network_id: set when using managed networking
    .bind(&n.id)
    .bind(&n.tap)
    .bind(&n.mac)
    .bind(&n.host_mac)
    .bind(&n.ip)
    .bind(n.mtu.unwrap_or(1500))
    .bind(interface_type)
    .bind(n.vhost_user.unwrap_or(false))
    .bind(&n.vhost_socket)
    .bind(vhost_mode)
    .bind(n.num_queues.unwrap_or(1))
    .bind(n.queue_size.unwrap_or(256))
    .bind(rate_limiter)
    .bind(n.offload_tso.unwrap_or(true))
    .bind(n.offload_ufo.unwrap_or(true))
    .bind(n.offload_csum.unwrap_or(true))
    .bind(n.pci_segment.unwrap_or(0))
    .bind(n.iommu.unwrap_or(false))
    .execute(tx.as_mut())
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, interface_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM network_interfaces WHERE id = $1")
        .bind(interface_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_by_device_id(
    pool: &PgPool,
    vm_id: Uuid,
    device_id: &str,
) -> Result<Option<NetworkInterface>, sqlx::Error> {
    let row: Option<NetworkInterfaceRow> = sqlx::query_as!(
        NetworkInterfaceRow,
        r#"
SELECT id,
        vm_id,
        network_id as "network_id?",
        device_id as "device_id!",
        tap_name as "tap_name?",
        mac_address::text as "mac_address?",
        host_mac::text as "host_mac?",
        ip_address::text as "ip_address?",
        mtu as "mtu!",
        interface_type as "interface_type: _",
        vhost_user as "vhost_user!",
        vhost_socket as "vhost_socket?",
        vhost_mode as "vhost_mode?",
        num_queues as "num_queues!",
        queue_size as "queue_size!",
        rate_limiter as "rate_limiter: _",
        offload_tso as "offload_tso!",
        offload_ufo as "offload_ufo!",
        offload_csum as "offload_csum!",
        pci_segment as "pci_segment!",
        iommu as "iommu!"
FROM network_interfaces
WHERE vm_id = $1 AND device_id = $2
        "#,
        vm_id,
        device_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.into()))
}
