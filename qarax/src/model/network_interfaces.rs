use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type, types::Json};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::model::vms::NewVmNetwork;

pub type PgTransaction<'a> = Transaction<'a, Postgres>;

/// I/O limits for a disk or NIC, each a token bucket. On a NIC both apply to
/// received and sent traffic separately.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, Validate)]
pub struct RateLimiterConfig {
    /// Bytes per refill period.
    #[validate]
    pub bandwidth: Option<TokenBucket>,
    /// Operations per refill period: disk requests, or NIC packets.
    #[validate]
    pub ops: Option<TokenBucket>,
}

/// A token bucket holding up to `size` tokens, refilled from empty in
/// `refill_time` milliseconds, so it allows `size` per `refill_time` on
/// average. It starts full.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, Validate)]
pub struct TokenBucket {
    /// Bucket capacity, in bytes or operations.
    #[validate(range(min = 1))]
    #[schema(minimum = 1)]
    pub size: i64,
    /// Milliseconds the bucket takes to refill from empty.
    #[validate(range(min = 1))]
    #[schema(minimum = 1)]
    pub refill_time: i64,
    /// Tokens available once, on top of the bucket, before the limit applies.
    #[validate(range(min = 0))]
    #[schema(minimum = 0)]
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
    pub rate_limiter: Option<RateLimiterConfig>,

    // Offload features
    pub offload_tso: bool,
    pub offload_ufo: bool,
    pub offload_csum: bool,

    // PCI configuration
    pub pci_segment: i32,
    pub iommu: bool,
}

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
    pub rate_limiter: Option<Json<RateLimiterConfig>>,
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

pub async fn list_by_vm<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    vm_id: Uuid,
) -> Result<Vec<NetworkInterface>, sqlx::Error> {
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
    .fetch_all(executor)
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

    // network_id is set when using managed networking.
    sqlx::query!(
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
    $5, $6::text::macaddr, $7::text::macaddr, $8::text::inet, $9,
    $10, $11, $12, $13,
    $14, $15, $16,
    $17, $18, $19,
    $20, $21
)
        "#,
        id,
        vm_id,
        n.network_id,
        &n.id,
        n.tap.as_deref(),
        n.mac.as_deref(),
        n.host_mac.as_deref(),
        n.ip.as_deref(),
        n.mtu.unwrap_or(1500),
        interface_type as InterfaceType,
        n.vhost_user.unwrap_or(false),
        n.vhost_socket.as_deref(),
        vhost_mode,
        n.num_queues.unwrap_or(1),
        n.queue_size.unwrap_or(256),
        n.rate_limiter.as_ref().map(Json) as Option<Json<&RateLimiterConfig>>,
        n.offload_tso.unwrap_or(true),
        n.offload_ufo.unwrap_or(true),
        n.offload_csum.unwrap_or(true),
        n.pci_segment.unwrap_or(0),
        n.iommu.unwrap_or(false)
    )
    .execute(tx.as_mut())
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, interface_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM network_interfaces WHERE id = $1", interface_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_tx(tx: &mut PgTransaction<'_>, interface_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM network_interfaces WHERE id = $1", interface_id)
        .execute(tx.as_mut())
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

/// Give `fork_vm_id` a copy of NIC `source_nic_id`, on its network with
/// `ip_address`, the fork's. It has none of the source's MAC, host MAC or
/// TAP: the node gives the fork its own.
pub async fn copy_for_fork_tx(
    tx: &mut PgTransaction<'_>,
    source_nic_id: Uuid,
    fork_vm_id: Uuid,
    ip_address: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let inserted = sqlx::query!(
        r#"
INSERT INTO network_interfaces (
    id, vm_id, network_id, device_id, ip_address, mtu,
    interface_type, vhost_user, vhost_socket, vhost_mode,
    num_queues, queue_size, rate_limiter,
    offload_tso, offload_ufo, offload_csum,
    pci_segment, iommu
)
SELECT
    $1, $2, network_id, device_id, $3::text::inet, mtu,
    interface_type, vhost_user, vhost_socket, vhost_mode,
    num_queues, queue_size, rate_limiter,
    offload_tso, offload_ufo, offload_csum,
    pci_segment, iommu
FROM network_interfaces
WHERE id = $4
        "#,
        id,
        fork_vm_id,
        ip_address,
        source_nic_id
    )
    .execute(tx.as_mut())
    .await?
    .rows_affected();
    if inserted == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::{copy_for_fork_tx, create, list_by_vm};
    use crate::model::{
        networks::{self, NewNetwork},
        vms::{self, NewVmNetwork},
    };
    use crate::test_support::{TestDatabase, resolved_vm};

    #[tokio::test]
    async fn a_forks_nic_is_its_sources_with_the_forks_address() {
        let db = TestDatabase::new().await;
        let network_id = networks::create(
            &db.pool,
            NewNetwork {
                name: "isolated".into(),
                subnet: "10.9.0.0/24".into(),
                gateway: Some("10.9.0.1".into()),
                dns: None,
                vpc_name: None,
                network_type: Some("isolated".into()),
            },
        )
        .await
        .unwrap();
        let mut tx = db.pool.begin().await.unwrap();
        let source = vms::create_tx(&mut tx, &resolved_vm("source"), None)
            .await
            .unwrap();
        let fork = vms::create_tx(&mut tx, &resolved_vm("fork"), None)
            .await
            .unwrap();
        let nic = create(
            &mut tx,
            source,
            &NewVmNetwork {
                id: "net0".into(),
                network_id: Some(network_id),
                mac: Some("02:00:00:00:00:01".into()),
                tap: Some("qfsource0".into()),
                ip: Some("10.9.0.2".into()),
                mtu: Some(1400),
                host_mac: Some("02:00:00:00:01:01".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        copy_for_fork_tx(&mut tx, nic, fork, "10.9.0.3")
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let copies = list_by_vm(&db.pool, fork).await.unwrap();
        let [copy] = copies.as_slice() else {
            panic!("{} NICs", copies.len());
        };
        assert_eq!(
            (copy.device_id.as_str(), copy.network_id, copy.mtu),
            ("net0", Some(network_id), 1400)
        );
        assert_eq!(
            copy.ip_address
                .as_deref()
                .map(|ip| ip.split('/').next().unwrap()),
            Some("10.9.0.3")
        );
        assert_eq!(
            (&copy.mac_address, &copy.host_mac, &copy.tap_name),
            (&None, &None, &None)
        );
    }
}
