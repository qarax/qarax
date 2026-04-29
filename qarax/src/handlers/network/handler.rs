use super::*;
use crate::{
    App,
    grpc_client::NodeClient,
    model::{
        hosts,
        networks::{self, IpAllocation, Network, NewNetwork},
    },
    network_policy,
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use serde::Deserialize;
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/networks",
    params(crate::handlers::NameQuery),
    responses(
        (status = 200, description = "List all networks", body = Vec<Network>),
        (status = 500, description = "Internal server error")
    ),
    tag = "networks"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::NameQuery>,
) -> Result<ApiResponse<Vec<Network>>> {
    let nets = networks::list(env.pool(), query.name.as_deref()).await?;
    Ok(ApiResponse {
        data: nets,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/networks/{network_id}",
    params(
        ("network_id" = uuid::Uuid, Path, description = "Network unique identifier")
    ),
    responses(
        (status = 200, description = "Network found", body = Network),
        (status = 404, description = "Network not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "networks"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(network_id): Path<Uuid>,
) -> Result<ApiResponse<Network>> {
    let net = networks::get(env.pool(), network_id).await?;
    Ok(ApiResponse {
        data: net,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/networks",
    request_body = NewNetwork,
    responses(
        (status = 201, description = "Network created successfully", body = String),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "networks"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(new_net): Json<NewNetwork>,
) -> Result<(StatusCode, String)> {
    let id = networks::create(env.pool(), new_net).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    delete,
    path = "/networks/{network_id}",
    params(
        ("network_id" = uuid::Uuid, Path, description = "Network unique identifier")
    ),
    responses(
        (status = 204, description = "Network deleted successfully"),
        (status = 404, description = "Network not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "networks"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(network_id): Path<Uuid>,
) -> Result<StatusCode> {
    networks::delete(env.pool(), network_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AttachHostRequest {
    pub host_id: Uuid,
    pub bridge_name: String,
    #[serde(default)]
    pub parent_interface: Option<String>,
}

#[utoipa::path(
    post,
    path = "/networks/{network_id}/hosts",
    params(
        ("network_id" = uuid::Uuid, Path, description = "Network unique identifier")
    ),
    request_body = AttachHostRequest,
    responses(
        (status = 204, description = "Host attached to network"),
        (status = 404, description = "Network or host not found"),
        (status = 422, description = "Failed to attach network on node"),
        (status = 500, description = "Internal server error")
    ),
    tag = "networks"
)]
#[instrument(skip(env))]
pub async fn attach_host(
    Extension(env): Extension<App>,
    Path(network_id): Path<Uuid>,
    Json(body): Json<AttachHostRequest>,
) -> Result<StatusCode> {
    let network = networks::get(env.pool(), network_id).await?;
    let host = hosts::require_by_id(env.pool(), body.host_id).await?;

    // passt-backed networks don't require bridge/DHCP/NAT provisioning.
    if network.network_type.as_deref() == Some("passt") {
        networks::attach_host(env.pool(), network_id, body.host_id, &body.bridge_name).await?;
        network_policy::sync_cluster_vpc_state(&env, &network, &[body.host_id]).await?;
        return Ok(StatusCode::NO_CONTENT);
    }

    let parent_interface = body.parent_interface.clone().unwrap_or_default();

    // Both isolated and bridged modes need DHCP range (for the DHCP server to serve VMs).
    // Bridged mode skips NAT but still needs DHCP.
    let (dhcp_start, dhcp_end) = compute_dhcp_range(&network.subnet, network.gateway.as_deref());

    let gateway = network
        .gateway
        .clone()
        .unwrap_or_else(|| default_gateway(&network.subnet));

    let dns = network.dns.clone().unwrap_or_else(|| gateway.clone());

    // Call gRPC to set up bridge on the node
    let client = NodeClient::new(&host.address, host.port as u16);
    client
        .attach_network(
            &body.bridge_name,
            &network.subnet,
            &gateway,
            &dns,
            &dhcp_start,
            &dhcp_end,
            &parent_interface,
        )
        .await
        .map_err(|e| {
            tracing::error!(
                network_id = %network_id,
                host_id = %body.host_id,
                error = %e,
                "gRPC attach_network failed"
            );
            crate::errors::Error::UnprocessableEntity(format!(
                "Failed to attach network to node: {e}"
            ))
        })?;

    // Record the attachment in the DB
    networks::attach_host(env.pool(), network_id, body.host_id, &body.bridge_name).await?;
    network_policy::sync_cluster_vpc_state(&env, &network, &[body.host_id]).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/networks/{network_id}/hosts/{host_id}",
    params(
        ("network_id" = uuid::Uuid, Path, description = "Network unique identifier"),
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    responses(
        (status = 204, description = "Host detached from network"),
        (status = 404, description = "Host not found"),
        (status = 422, description = "Failed to detach network on node"),
        (status = 500, description = "Internal server error")
    ),
    tag = "networks"
)]
#[instrument(skip(env))]
pub async fn detach_host(
    Extension(env): Extension<App>,
    Path((network_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode> {
    let host = hosts::require_by_id(env.pool(), host_id).await?;
    let network = networks::get(env.pool(), network_id).await?;

    let bridge_name = networks::get_host_bridge(env.pool(), host_id, network_id)
        .await?
        .ok_or(crate::errors::Error::NotFound)?;

    // Call gRPC to tear down bridge on the node
    let client = NodeClient::new(&host.address, host.port as u16);
    client
        .detach_network(&bridge_name, &network.subnet)
        .await
        .map_err(|e| {
            tracing::error!(
                network_id = %network_id,
                host_id = %host_id,
                error = %e,
                "gRPC detach_network failed"
            );
            crate::errors::Error::UnprocessableEntity(format!(
                "Failed to detach network from node: {e}"
            ))
        })?;

    // Remove the DB record
    networks::detach_host(env.pool(), network_id, host_id).await?;
    network_policy::sync_cluster_vpc_state(&env, &network, &[host_id]).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/networks/{network_id}/ips",
    params(
        ("network_id" = uuid::Uuid, Path, description = "Network unique identifier")
    ),
    responses(
        (status = 200, description = "List IP allocations", body = Vec<IpAllocation>),
        (status = 500, description = "Internal server error")
    ),
    tag = "networks"
)]
#[instrument(skip(env))]
pub async fn list_ips(
    Extension(env): Extension<App>,
    Path(network_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<IpAllocation>>> {
    let allocations = networks::list_allocations(env.pool(), network_id).await?;
    Ok(ApiResponse {
        data: allocations,
        code: StatusCode::OK,
    })
}

/// Compute a default gateway from a CIDR (first usable address).
fn default_gateway(subnet: &str) -> String {
    if let Some((base, _prefix)) = subnet.split_once('/') {
        let octets: Vec<u8> = base.split('.').filter_map(|o| o.parse().ok()).collect();
        if octets.len() == 4 {
            return format!(
                "{}.{}.{}.{}",
                octets[0],
                octets[1],
                octets[2],
                octets[3] + 1
            );
        }
    }
    "10.0.0.1".to_string()
}

/// Compute DHCP range: start at the first usable host after the gateway, end at broadcast-1.
/// The start must align with `next_available_ip` (which also starts at network_addr+1 and
/// skips the gateway) so the API-allocated IP matches what the DHCP server actually hands out.
fn compute_dhcp_range(subnet: &str, gateway: Option<&str>) -> (String, String) {
    if let Some((base, prefix_str)) = subnet.split_once('/') {
        let octets: Vec<u8> = base.split('.').filter_map(|o| o.parse().ok()).collect();
        let prefix_len: u32 = prefix_str.parse().unwrap_or(24);
        if octets.len() == 4 {
            let base_u32 = (octets[0] as u32) << 24
                | (octets[1] as u32) << 16
                | (octets[2] as u32) << 8
                | octets[3] as u32;
            let host_bits = 32 - prefix_len;
            let network_addr = base_u32 & (u32::MAX << host_bits);
            let broadcast_addr = network_addr | ((1u32 << host_bits) - 1);

            // Parse the gateway IP so we can skip it when selecting the range start.
            let gw_u32: Option<u32> = gateway.and_then(|gw| {
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

            // Start at network_addr+1, skip the gateway — mirrors next_available_ip logic.
            let mut start = network_addr + 1;
            if gw_u32 == Some(start) {
                start += 1;
            }
            let end = broadcast_addr - 1; // .254 for /24

            let fmt = |addr: u32| {
                format!(
                    "{}.{}.{}.{}",
                    (addr >> 24) & 0xFF,
                    (addr >> 16) & 0xFF,
                    (addr >> 8) & 0xFF,
                    addr & 0xFF
                )
            };

            return (fmt(start), fmt(end));
        }
    }
    ("10.0.0.10".to_string(), "10.0.0.254".to_string())
}
