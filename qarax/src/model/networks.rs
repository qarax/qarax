use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Network {
    pub id: Uuid,
    pub name: String,
    pub subnet: String,
    pub gateway: Option<String>,
    pub dns: Option<String>,
    pub vpc_name: Option<String>,
    #[serde(rename = "type")]
    pub network_type: Option<String>,
    pub status: NetworkStatus,
}

struct NetworkRow {
    id: Uuid,
    name: String,
    subnet: String,
    gateway: Option<String>,
    dns: Option<String>,
    vpc_name: Option<String>,
    network_type: Option<String>,
    status: NetworkStatus,
}

impl From<NetworkRow> for Network {
    fn from(row: NetworkRow) -> Self {
        fn normalize_inet(value: Option<String>) -> Option<String> {
            value.map(|v| v.split('/').next().unwrap_or(&v).to_string())
        }

        Network {
            id: row.id,
            name: row.name,
            subnet: row.subnet,
            gateway: normalize_inet(row.gateway),
            dns: normalize_inet(row.dns),
            vpc_name: row.vpc_name,
            network_type: row.network_type,
            status: row.status,
        }
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "network_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum NetworkStatus {
    Active,
    Inactive,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewNetwork {
    pub name: String,
    pub subnet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_name: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub network_type: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct IpAllocation {
    pub id: Uuid,
    pub network_id: Uuid,
    pub ip_address: String,
    pub vm_id: Option<Uuid>,
    pub allocated_at: chrono::DateTime<chrono::Utc>,
}

/// A host a network is attached to, and the bridge carrying it there.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NetworkHost {
    pub host_id: Uuid,
    pub host_name: String,
    pub bridge_name: String,
}

#[derive(Debug, Clone)]
pub struct AttachedNetwork {
    pub host_id: Uuid,
    pub host_address: String,
    pub host_port: i32,
    pub network: Network,
    pub bridge_name: String,
}

struct IpAllocationRow {
    id: Uuid,
    network_id: Uuid,
    ip_address: String,
    vm_id: Option<Uuid>,
    allocated_at: chrono::DateTime<chrono::Utc>,
}

impl From<IpAllocationRow> for IpAllocation {
    fn from(row: IpAllocationRow) -> Self {
        IpAllocation {
            id: row.id,
            network_id: row.network_id,
            ip_address: row.ip_address,
            vm_id: row.vm_id,
            allocated_at: row.allocated_at,
        }
    }
}

pub type PgTransaction<'a> = Transaction<'a, Postgres>;

/// A network's subnet. Networks are IPv4 (nodes' bridges, DHCP and firewall
/// are), and the subnet column is a normalized CIDR such as "10.0.0.0/24".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv4Subnet {
    network: u32,
    prefix: u32,
}

impl Ipv4Subnet {
    pub fn parse(cidr: &str) -> Option<Self> {
        let (base, prefix) = cidr.split_once('/')?;
        let base: std::net::Ipv4Addr = base.parse().ok()?;
        let prefix = prefix.parse().ok().filter(|prefix| *prefix <= 32)?;
        let subnet = Self { network: 0, prefix };
        Some(Self {
            network: u32::from(base) & subnet.mask(),
            prefix,
        })
    }

    fn mask(&self) -> u32 {
        u32::MAX.checked_shl(32 - self.prefix).unwrap_or(0)
    }

    pub fn netmask(&self) -> std::net::Ipv4Addr {
        self.mask().into()
    }

    /// The address after the network address: the default gateway.
    pub fn first_host(&self) -> std::net::Ipv4Addr {
        self.network.wrapping_add(1).into()
    }
}

// CRUD

pub async fn list(pool: &PgPool, name_filter: Option<&str>) -> Result<Vec<Network>, sqlx::Error> {
    let rows: Vec<NetworkRow> = sqlx::query_as!(
        NetworkRow,
        r#"
SELECT id, name, subnet::text AS "subnet!", gateway::text, dns::text, vpc_name, type AS network_type, status AS "status: _"
FROM networks
WHERE ($1::text IS NULL OR name = $1)
        "#,
        name_filter,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get(pool: &PgPool, network_id: Uuid) -> Result<Network, sqlx::Error> {
    let row: NetworkRow = sqlx::query_as!(
        NetworkRow,
        r#"
SELECT id, name, subnet::text AS "subnet!", gateway::text, dns::text, vpc_name, type AS network_type, status AS "status: _"
FROM networks
WHERE id = $1
        "#,
        network_id,
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn create(pool: &PgPool, new: NewNetwork) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query!(
        r#"
INSERT INTO networks (id, name, subnet, gateway, dns, vpc_name, type, status)
VALUES ($1, $2, $3::text::cidr, $4::text::inet, $5::text::inet, $6, $7, $8)
        "#,
        id,
        &new.name,
        &new.subnet,
        new.gateway.as_deref(),
        new.dns.as_deref(),
        new.vpc_name.as_deref(),
        new.network_type.as_deref(),
        NetworkStatus::Active as NetworkStatus
    )
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, network_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM networks WHERE id = $1", network_id)
        .execute(pool)
        .await?;

    Ok(())
}

// Host binding

pub async fn attach_host(
    pool: &PgPool,
    network_id: Uuid,
    host_id: Uuid,
    bridge_name: &str,
    parent_interface: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
INSERT INTO host_networks (host_id, network_id, bridge_name, parent_interface)
VALUES ($1, $2, $3, $4)
ON CONFLICT DO NOTHING
        "#,
        host_id,
        network_id,
        bridge_name,
        parent_interface
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn detach_host(
    pool: &PgPool,
    network_id: Uuid,
    host_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM host_networks WHERE host_id = $1 AND network_id = $2",
        host_id,
        network_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn host_has_network(
    pool: &PgPool,
    host_id: Uuid,
    network_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query_scalar!(
        "SELECT COUNT(*) AS \"count!\" FROM host_networks WHERE host_id = $1 AND network_id = $2",
        host_id,
        network_id
    )
    .fetch_one(pool)
    .await?;

    Ok(row > 0)
}

/// Hosts the network is attached to, by host name.
pub async fn list_hosts(pool: &PgPool, network_id: Uuid) -> Result<Vec<NetworkHost>, sqlx::Error> {
    sqlx::query_as!(
        NetworkHost,
        r#"
SELECT hn.host_id, h.name AS host_name, hn.bridge_name
FROM host_networks hn
JOIN hosts h ON h.id = hn.host_id
WHERE hn.network_id = $1
ORDER BY h.name
        "#,
        network_id
    )
    .fetch_all(pool)
    .await
}

/// What still uses a network: "vm <name>" for VMs with a NIC on it and
/// "template <name>" for VM templates that create VMs on it.
pub async fn users(pool: &PgPool, network_id: Uuid) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
SELECT DISTINCT 'vm ' || v.name AS "user!"
FROM vms v
JOIN network_interfaces ni ON ni.vm_id = v.id
WHERE ni.network_id = $1
UNION
SELECT 'template ' || t.name
FROM vm_templates t
WHERE t.network_id = $1
ORDER BY 1
        "#,
        network_id
    )
    .fetch_all(pool)
    .await
}

/// Return the bridge name for a network on a specific host.
pub async fn get_host_bridge(
    pool: &PgPool,
    host_id: Uuid,
    network_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query_scalar!(
        "SELECT bridge_name FROM host_networks WHERE host_id = $1 AND network_id = $2",
        host_id,
        network_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// A network attached to a host, with what re-creating it there needs.
pub struct HostAttachment {
    pub network: Network,
    pub bridge_name: String,
    pub parent_interface: Option<String>,
}

pub async fn list_for_host(
    pool: &PgPool,
    host_id: Uuid,
) -> Result<Vec<HostAttachment>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
SELECT n.id,
       n.name,
       n.subnet::text AS "subnet!",
       n.gateway::text AS gateway,
       n.dns::text AS dns,
       n.vpc_name,
       n.type AS network_type,
       n.status AS "status: NetworkStatus",
       hn.bridge_name,
       hn.parent_interface
FROM networks n
JOIN host_networks hn ON hn.network_id = n.id
WHERE hn.host_id = $1
ORDER BY n.name
            "#,
        host_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| HostAttachment {
            network: Network {
                id: r.id,
                name: r.name,
                subnet: r.subnet,
                gateway: r
                    .gateway
                    .map(|v| v.split('/').next().unwrap_or(&v).to_string()),
                dns: r.dns.map(|v| v.split('/').next().unwrap_or(&v).to_string()),
                vpc_name: r.vpc_name,
                network_type: r.network_type,
                status: r.status,
            },
            bridge_name: r.bridge_name,
            parent_interface: r.parent_interface,
        })
        .collect())
}

pub async fn list_attached(pool: &PgPool) -> Result<Vec<AttachedNetwork>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
SELECT n.id,
       n.name,
       n.subnet::text AS "subnet!",
       n.gateway::text AS gateway,
       n.dns::text AS dns,
       n.vpc_name,
       n.type AS network_type,
       n.status AS "status: NetworkStatus",
       h.id AS host_id,
       h.address AS host_address,
       h.port AS host_port,
       hn.bridge_name
FROM networks n
JOIN host_networks hn ON hn.network_id = n.id
JOIN hosts h ON h.id = hn.host_id
ORDER BY h.id, n.name
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| AttachedNetwork {
            host_id: r.host_id,
            host_address: r.host_address,
            host_port: r.host_port,
            bridge_name: r.bridge_name,
            network: Network {
                id: r.id,
                name: r.name,
                subnet: r.subnet,
                gateway: r
                    .gateway
                    .map(|v| v.split('/').next().unwrap_or(&v).to_string()),
                dns: r.dns.map(|v| v.split('/').next().unwrap_or(&v).to_string()),
                vpc_name: r.vpc_name,
                network_type: r.network_type,
                status: r.status,
            },
        })
        .collect())
}

pub async fn list_vpc_host_ids(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows = sqlx::query_scalar!(
        r#"
SELECT DISTINCT hn.host_id
FROM host_networks hn
JOIN networks n ON n.id = hn.network_id
WHERE n.vpc_name IS NOT NULL
ORDER BY hn.host_id
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

// IPAM

/// Allocate an IP on a network to a VM: `requested` when it's free, otherwise
/// the lowest free host address (never the network, broadcast or gateway
/// address). Returns None when `requested` is taken or the network is full.
///
/// Allocations on one network are serialized by locking its row for the rest
/// of the transaction, so a concurrent allocation waits instead of racing, and
/// a second allocation in the same transaction sees the first.
pub async fn allocate_ip_tx(
    tx: &mut PgTransaction<'_>,
    network_id: Uuid,
    requested: Option<&str>,
    vm_id: Uuid,
) -> Result<Option<IpAllocation>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT id FROM networks WHERE id = $1 FOR NO KEY UPDATE",
        network_id
    )
    .fetch_one(tx.as_mut())
    .await?;

    let row = match requested {
        Some(ip_address) => {
            sqlx::query_as!(
                IpAllocationRow,
                r#"
INSERT INTO ip_allocations (network_id, ip_address, vm_id)
VALUES ($1, $2::text::inet, $3)
ON CONFLICT (network_id, ip_address) DO NOTHING
RETURNING id, network_id, ip_address::text AS "ip_address!", vm_id, allocated_at
                "#,
                network_id,
                ip_address,
                vm_id
            )
            .fetch_optional(tx.as_mut())
            .await?
        }
        // The lowest free address follows the network address, an allocated
        // address or the gateway, so those are the only candidates to check.
        None => {
            sqlx::query_as!(
                IpAllocationRow,
                r#"
WITH net AS (
    SELECT id, subnet,
           host(network(subnet))::inet AS first,
           host(broadcast(subnet))::inet AS last,
           host(gateway)::inet AS gateway
    FROM networks
    WHERE id = $1 AND family(subnet) = 4
),
taken AS (
    SELECT host(a.ip_address)::inet AS ip
    FROM ip_allocations a, net
    WHERE a.network_id = net.id
),
candidate AS (
    SELECT c.ip
    FROM net, LATERAL (
        SELECT net.first + 1 AS ip
        UNION SELECT ip + 1 FROM taken
        UNION SELECT net.gateway + 1 WHERE net.gateway IS NOT NULL
    ) c
    WHERE c.ip << net.subnet
      AND c.ip > net.first
      AND c.ip < net.last
      AND c.ip IS DISTINCT FROM net.gateway
      AND c.ip NOT IN (SELECT ip FROM taken)
    ORDER BY c.ip
    LIMIT 1
)
INSERT INTO ip_allocations (network_id, ip_address, vm_id)
SELECT $1, ip, $2 FROM candidate
RETURNING id, network_id, ip_address::text AS "ip_address!", vm_id, allocated_at
                "#,
                network_id,
                vm_id
            )
            .fetch_optional(tx.as_mut())
            .await?
        }
    };

    Ok(row.map(Into::into))
}

pub async fn list_allocations_by_vm(
    pool: &PgPool,
    vm_id: Uuid,
) -> Result<Vec<IpAllocation>, sqlx::Error> {
    let rows = sqlx::query_as!(
        IpAllocationRow,
        r#"
SELECT id, network_id, ip_address::text AS "ip_address!", vm_id, allocated_at
FROM ip_allocations
WHERE vm_id = $1
        "#,
        vm_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get_allocation(
    pool: &PgPool,
    network_id: Uuid,
    ip_address: &str,
) -> Result<Option<IpAllocation>, sqlx::Error> {
    let row = sqlx::query_as!(
        IpAllocationRow,
        r#"
SELECT id, network_id, ip_address::text AS "ip_address!", vm_id, allocated_at
FROM ip_allocations
WHERE network_id = $1 AND ip_address = $2::text::inet
        "#,
        network_id,
        ip_address
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Into::into))
}

pub async fn release_ip(pool: &PgPool, allocation_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM ip_allocations WHERE id = $1", allocation_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn release_ip_by_network_and_address(
    pool: &PgPool,
    network_id: Uuid,
    ip_address: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM ip_allocations WHERE network_id = $1 AND ip_address = $2::text::inet",
        network_id,
        ip_address
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn release_ip_by_network_and_address_tx(
    tx: &mut PgTransaction<'_>,
    network_id: Uuid,
    ip_address: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM ip_allocations WHERE network_id = $1 AND ip_address = $2::text::inet",
        network_id,
        ip_address
    )
    .execute(tx.as_mut())
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn list_allocations(
    pool: &PgPool,
    network_id: Uuid,
) -> Result<Vec<IpAllocation>, sqlx::Error> {
    let rows = sqlx::query_as!(
        IpAllocationRow,
        r#"
SELECT id, network_id, ip_address::text AS "ip_address!", vm_id, allocated_at
FROM ip_allocations
WHERE network_id = $1
ORDER BY ip_address
        "#,
        network_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{Ipv4Subnet, NewNetwork, allocate_ip_tx, release_ip_by_network_and_address};
    use crate::{
        model::vms,
        test_support::{TestDatabase, resolved_vm},
    };

    async fn add_network(db: &TestDatabase, subnet: &str, gateway: Option<&str>) -> Uuid {
        super::create(
            &db.pool,
            NewNetwork {
                name: format!("net-{}", Uuid::new_v4()),
                subnet: subnet.to_string(),
                gateway: gateway.map(str::to_string),
                dns: None,
                vpc_name: None,
                network_type: None,
            },
        )
        .await
        .expect("Failed to create network")
    }

    async fn add_vm(db: &TestDatabase) -> Uuid {
        let mut tx = db.pool.begin().await.unwrap();
        let vm_id = vms::create_tx(&mut tx, &resolved_vm(&Uuid::new_v4().to_string()), None)
            .await
            .expect("Failed to create VM");
        tx.commit().await.unwrap();
        vm_id
    }

    fn host(allocation: Option<super::IpAllocation>) -> Option<String> {
        allocation.map(|a| a.ip_address.trim_end_matches("/32").to_string())
    }

    /// Allocate without a requested address in a transaction of its own.
    async fn allocate(db: &TestDatabase, network_id: Uuid, vm_id: Uuid) -> Option<String> {
        let mut tx = db.pool.begin().await.unwrap();
        let allocation = allocate_ip_tx(&mut tx, network_id, None, vm_id)
            .await
            .expect("allocation failed");
        tx.commit().await.unwrap();
        host(allocation)
    }

    #[test]
    fn ipv4_subnets_give_their_mask_and_first_host() {
        let subnet = Ipv4Subnet::parse("10.1.2.0/23").unwrap();
        assert_eq!(subnet.netmask().to_string(), "255.255.254.0");
        assert_eq!(subnet.first_host().to_string(), "10.1.2.1");
        assert_eq!(
            Ipv4Subnet::parse("10.0.0.0/0")
                .unwrap()
                .netmask()
                .to_string(),
            "0.0.0.0"
        );
        for not_ipv4 in ["fd00::/64", "10.0.0.0", "10.0.0.0/33"] {
            assert_eq!(Ipv4Subnet::parse(not_ipv4), None, "{not_ipv4}");
        }
    }

    #[tokio::test]
    async fn allocation_takes_the_lowest_free_address_around_the_gateway() {
        let db = TestDatabase::new().await;
        // Hosts .1 to .6, with the gateway in the middle.
        let network_id = add_network(&db, "10.9.0.0/29", Some("10.9.0.3")).await;
        let vm_id = add_vm(&db).await;

        let mut allocated = Vec::new();
        while let Some(ip) = allocate(&db, network_id, vm_id).await {
            allocated.push(ip);
        }
        assert_eq!(
            allocated,
            ["10.9.0.1", "10.9.0.2", "10.9.0.4", "10.9.0.5", "10.9.0.6"]
        );

        release_ip_by_network_and_address(&db.pool, network_id, "10.9.0.2")
            .await
            .unwrap();
        assert_eq!(
            allocate(&db, network_id, vm_id).await.as_deref(),
            Some("10.9.0.2")
        );
    }

    #[tokio::test]
    async fn allocations_in_one_transaction_see_each_other() {
        // VM create registers a requested address and then allocates the
        // managed NIC's in the same transaction.
        let db = TestDatabase::new().await;
        let network_id = add_network(&db, "10.9.1.0/24", Some("10.9.1.254")).await;
        let vm_id = add_vm(&db).await;

        let mut tx = db.pool.begin().await.unwrap();
        let requested = allocate_ip_tx(&mut tx, network_id, Some("10.9.1.1"), vm_id)
            .await
            .unwrap();
        let managed = allocate_ip_tx(&mut tx, network_id, None, vm_id)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(host(requested).as_deref(), Some("10.9.1.1"));
        assert_eq!(host(managed).as_deref(), Some("10.9.1.2"));
    }

    #[tokio::test]
    async fn a_requested_address_that_is_taken_is_not_allocated() {
        let db = TestDatabase::new().await;
        let network_id = add_network(&db, "10.9.2.0/24", None).await;
        let (first, second) = (add_vm(&db).await, add_vm(&db).await);

        for (vm_id, expected) in [(first, Some("10.9.2.7")), (second, None)] {
            let mut tx = db.pool.begin().await.unwrap();
            let allocation = allocate_ip_tx(&mut tx, network_id, Some("10.9.2.7"), vm_id)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            assert_eq!(host(allocation).as_deref(), expected);
        }
    }
}
