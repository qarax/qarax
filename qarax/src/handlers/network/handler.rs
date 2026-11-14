use super::*;
use crate::{
    App,
    grpc_client::NodeClient,
    model::{
        hosts,
        hosts::HostStatus,
        networks::{self, IpAllocation, Ipv4Subnet, Network, NetworkHost, NewNetwork},
    },
    network_policy,
};
use anyhow::Context;
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use serde::Deserialize;
use tracing::{instrument, warn};
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
        (status = 204, description = "Network detached from its hosts and deleted"),
        (status = 404, description = "Network not found"),
        (status = 409, description = "Network is in use by VMs or VM templates"),
        (status = 422, description = "Failed to detach the network on a node"),
        (status = 500, description = "Internal server error")
    ),
    tag = "networks"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(network_id): Path<Uuid>,
) -> Result<StatusCode> {
    let network = networks::get(env.pool(), network_id).await?;
    let users = networks::users(env.pool(), network_id).await?;
    if !users.is_empty() {
        return Err(crate::errors::Error::Conflict(format!(
            "Network {} is in use by {}",
            network.name,
            users.join(", ")
        )));
    }

    // Tear the network down on every host first, as detach_host does, so no
    // bridge, DHCP server or NAT rule outlives it on a node.
    let attachments = networks::list_hosts(env.pool(), network_id).await?;
    for attachment in &attachments {
        let host = hosts::require_by_id(env.pool(), attachment.host_id).await?;
        if host.status == HostStatus::Up {
            detach_on_node(&host, &network, &attachment.bridge_name).await?;
        } else {
            // A node that isn't up can't be reached; drop the record anyway
            // rather than leave the network undeletable.
            warn!(
                network_id = %network_id,
                host_id = %host.id,
                status = %host.status,
                "Host is not up; deleting the network without tearing down its bridge there"
            );
        }
        networks::detach_host(env.pool(), network_id, host.id).await?;
    }

    networks::delete(env.pool(), network_id).await?;
    let host_ids: Vec<Uuid> = attachments.iter().map(|a| a.host_id).collect();
    network_policy::sync_cluster_vpc_state(&env, &network, &host_ids).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/networks/{network_id}/hosts",
    params(
        ("network_id" = uuid::Uuid, Path, description = "Network unique identifier")
    ),
    responses(
        (status = 200, description = "Hosts the network is attached to", body = Vec<NetworkHost>),
        (status = 404, description = "Network not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "networks"
)]
#[instrument(skip(env))]
pub async fn list_hosts(
    Extension(env): Extension<App>,
    Path(network_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<NetworkHost>>> {
    networks::get(env.pool(), network_id).await?;
    Ok(ApiResponse {
        data: networks::list_hosts(env.pool(), network_id).await?,
        code: StatusCode::OK,
    })
}

/// Remove a network's bridge, DHCP server and NAT rules from a node.
async fn detach_on_node(host: &hosts::Host, network: &Network, bridge_name: &str) -> Result<()> {
    NodeClient::new(&host.address, host.port as u16)
        .detach_network(bridge_name, &network.subnet)
        .await
        .map_err(|e| {
            tracing::error!(
                network_id = %network.id,
                host_id = %host.id,
                error = %e,
                "gRPC detach_network failed"
            );
            crate::errors::Error::UnprocessableEntity(format!(
                "Failed to detach network from node: {e}"
            ))
        })
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

    let parent_interface = body.parent_interface.as_deref();
    // passt-backed networks don't require bridge/DHCP/NAT provisioning.
    if network.network_type.as_deref() != Some("passt") {
        attach_on_node(&host, &network, &body.bridge_name, parent_interface)
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
    }

    networks::attach_host(
        env.pool(),
        network_id,
        body.host_id,
        &body.bridge_name,
        parent_interface,
    )
    .await?;
    network_policy::sync_cluster_vpc_state(&env, &network, &[body.host_id]).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Create a network's bridge, DHCP server and NAT rules on a node. The node
/// treats an existing bridge as already set up, so this is safe to repeat.
async fn attach_on_node(
    host: &hosts::Host,
    network: &Network,
    bridge_name: &str,
    parent_interface: Option<&str>,
) -> anyhow::Result<()> {
    let gateway = match network.gateway.clone() {
        Some(gateway) => gateway,
        None => Ipv4Subnet::parse(&network.subnet)
            .with_context(|| format!("Network subnet {} is not IPv4", network.subnet))?
            .first_host()
            .to_string(),
    };
    let dns = network.dns.clone().unwrap_or_else(|| gateway.clone());

    NodeClient::new(&host.address, host.port as u16)
        .attach_network(
            bridge_name,
            &network.subnet,
            &gateway,
            &dns,
            parent_interface.unwrap_or_default(),
        )
        .await
}

/// Re-create every network attached to a host on its node, then its
/// isolation and VPC overlay state. Host init calls this: a node restart loses
/// its in-process DHCP servers, and a reboot its bridges and rules too.
/// Failures are logged; one network that can't be restored shouldn't keep
/// the others down.
pub async fn reattach_host_networks(env: &App, host_id: Uuid) {
    let host = match hosts::require_by_id(env.pool(), host_id).await {
        Ok(host) => host,
        Err(e) => {
            warn!(host_id = %host_id, error = %e, "Failed to load host to re-attach networks");
            return;
        }
    };
    let attachments = match networks::list_for_host(env.pool(), host_id).await {
        Ok(attachments) => attachments,
        Err(e) => {
            warn!(host_id = %host_id, error = %e, "Failed to list networks to re-attach");
            return;
        }
    };
    if attachments.is_empty() {
        return;
    }
    for attachment in &attachments {
        if attachment.network.network_type.as_deref() == Some("passt") {
            continue;
        }
        if let Err(e) = attach_on_node(
            &host,
            &attachment.network,
            &attachment.bridge_name,
            attachment.parent_interface.as_deref(),
        )
        .await
        {
            warn!(
                host_id = %host_id,
                network_id = %attachment.network.id,
                error = %e,
                "Failed to re-attach network on node"
            );
        }
    }
    if let Err(e) = network_policy::sync_host_network_isolation(env, host_id).await {
        warn!(host_id = %host_id, error = %e, "Failed to resync network isolation");
    }
    if let Err(e) = network_policy::sync_host_vpc_overlays(env, host_id).await {
        warn!(host_id = %host_id, error = %e, "Failed to resync VPC overlays");
    }
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

    detach_on_node(&host, &network, &bridge_name).await?;

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
