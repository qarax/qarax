use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type};
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
    #[serde(rename = "type")]
    pub network_type: Option<String>,
    pub status: NetworkStatus,
}

#[derive(sqlx::FromRow)]
struct NetworkRow {
    id: Uuid,
    name: String,
    subnet: String,
    gateway: Option<String>,
    dns: Option<String>,
    #[sqlx(rename = "type")]
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

#[derive(sqlx::FromRow)]
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

// CRUD

pub async fn list(pool: &PgPool, name_filter: Option<&str>) -> Result<Vec<Network>, sqlx::Error> {
    let rows: Vec<NetworkRow> = sqlx::query_as::<_, NetworkRow>(
        r#"
SELECT id, name, subnet::text, gateway::text, dns::text, type, status
FROM networks
WHERE ($1::text IS NULL OR name = $1)
        "#,
    )
    .bind(name_filter)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get(pool: &PgPool, network_id: Uuid) -> Result<Network, sqlx::Error> {
    let row: NetworkRow = sqlx::query_as::<_, NetworkRow>(
        r#"
SELECT id, name, subnet::text, gateway::text, dns::text, type, status
FROM networks
WHERE id = $1
        "#,
    )
    .bind(network_id)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn create(pool: &PgPool, new: NewNetwork) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query(
        r#"
INSERT INTO networks (id, name, subnet, gateway, dns, type, status)
VALUES ($1, $2, $3::cidr, $4::inet, $5::inet, $6, $7)
        "#,
    )
    .bind(id)
    .bind(&new.name)
    .bind(&new.subnet)
    .bind(&new.gateway)
    .bind(&new.dns)
    .bind(&new.network_type)
    .bind(NetworkStatus::Active)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, network_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM networks WHERE id = $1")
        .bind(network_id)
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
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
INSERT INTO host_networks (host_id, network_id, bridge_name)
VALUES ($1, $2, $3)
ON CONFLICT DO NOTHING
        "#,
    )
    .bind(host_id)
    .bind(network_id)
    .bind(bridge_name)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn detach_host(
    pool: &PgPool,
    network_id: Uuid,
    host_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM host_networks WHERE host_id = $1 AND network_id = $2")
        .bind(host_id)
        .bind(network_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn host_has_network(
    pool: &PgPool,
    host_id: Uuid,
    network_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM host_networks WHERE host_id = $1 AND network_id = $2",
    )
    .bind(host_id)
    .bind(network_id)
    .fetch_one(pool)
    .await?;

    Ok(row.0 > 0)
}

/// Return the bridge name for a network on a specific host.
pub async fn get_host_bridge(
    pool: &PgPool,
    host_id: Uuid,
    network_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT bridge_name FROM host_networks WHERE host_id = $1 AND network_id = $2",
    )
    .bind(host_id)
    .bind(network_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(name,)| name))
}

// IPAM

pub async fn allocate_ip(
    pool: &PgPool,
    network_id: Uuid,
    ip_address: &str,
    vm_id: Option<Uuid>,
) -> Result<IpAllocation, sqlx::Error> {
    let row = sqlx::query_as::<_, IpAllocationRow>(
        r#"
INSERT INTO ip_allocations (network_id, ip_address, vm_id)
VALUES ($1, $2::inet, $3)
ON CONFLICT (network_id, ip_address) DO UPDATE
    SET vm_id = EXCLUDED.vm_id, allocated_at = now()
RETURNING id, network_id, ip_address::text, vm_id, allocated_at
        "#,
    )
    .bind(network_id)
    .bind(ip_address)
    .bind(vm_id)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn release_ip(pool: &PgPool, allocation_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM ip_allocations WHERE id = $1")
        .bind(allocation_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn list_allocations(
    pool: &PgPool,
    network_id: Uuid,
) -> Result<Vec<IpAllocation>, sqlx::Error> {
    let rows = sqlx::query_as::<_, IpAllocationRow>(
        r#"
SELECT id, network_id, ip_address::text, vm_id, allocated_at
FROM ip_allocations
WHERE network_id = $1
ORDER BY ip_address
        "#,
    )
    .bind(network_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// Find the next available IP in the network's subnet.
/// Skips .0 (network), the gateway, and the broadcast address.
pub async fn next_available_ip(
    pool: &PgPool,
    network_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    let network = get(pool, network_id).await?;

    // Parse subnet CIDR
    let (base, prefix_len) = match network.subnet.split_once('/') {
        Some((ip, prefix)) => (ip.to_string(), prefix.parse::<u32>().unwrap_or(24)),
        None => return Ok(None),
    };

    let octets: Vec<u8> = base.split('.').filter_map(|o| o.parse().ok()).collect();
    if octets.len() != 4 {
        return Ok(None);
    }

    let base_u32 = (octets[0] as u32) << 24
        | (octets[1] as u32) << 16
        | (octets[2] as u32) << 8
        | octets[3] as u32;
    let host_bits = 32 - prefix_len;
    let network_addr = base_u32 & (u32::MAX << host_bits);
    let broadcast_addr = network_addr | ((1u32 << host_bits) - 1);

    // Get gateway IP as u32 for comparison
    let gateway_u32 = network.gateway.as_deref().and_then(|gw| {
        let parts: Vec<u8> = gw.split('.').filter_map(|o| o.parse().ok()).collect();
        if parts.len() == 4 {
            Some(
                (parts[0] as u32) << 24
                    | (parts[1] as u32) << 16
                    | (parts[2] as u32) << 8
                    | parts[3] as u32,
            )
        } else {
            None
        }
    });

    // Get already-allocated IPs
    let allocated: Vec<String> = sqlx::query_as::<_, (String,)>(
        "SELECT ip_address::text FROM ip_allocations WHERE network_id = $1",
    )
    .bind(network_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(ip,)| ip)
    .collect();

    let allocated_set: std::collections::HashSet<String> = allocated.into_iter().collect();

    // Iterate from network+1 to broadcast-1
    for addr in (network_addr + 1)..broadcast_addr {
        // Skip gateway
        if Some(addr) == gateway_u32 {
            continue;
        }

        let ip = format!(
            "{}.{}.{}.{}",
            (addr >> 24) & 0xFF,
            (addr >> 16) & 0xFF,
            (addr >> 8) & 0xFF,
            addr & 0xFF
        );

        // PostgreSQL INET may include /32 suffix — check both forms
        if !allocated_set.contains(&ip) && !allocated_set.contains(&format!("{}/32", ip)) {
            return Ok(Some(ip));
        }
    }

    Ok(None)
}
