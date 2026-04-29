use std::collections::{BTreeMap, BTreeSet, HashMap};

use futures::future::try_join_all;
use uuid::Uuid;

use crate::{
    App,
    errors::Error,
    grpc_client::{
        NodeClient,
        node::{
            FirewallDirection, FirewallProtocol, VmFirewallInterface, VmFirewallRule,
            VpcOverlayConfig, VpcOverlayRoute,
        },
    },
    model::{
        hosts, network_interfaces, networks,
        security_groups::{self, SecurityGroupDirection, SecurityGroupProtocol},
        vms::{self, VmStatus},
    },
};

fn firewall_direction_to_proto(direction: SecurityGroupDirection) -> i32 {
    match direction {
        SecurityGroupDirection::Ingress => FirewallDirection::Ingress as i32,
        SecurityGroupDirection::Egress => FirewallDirection::Egress as i32,
    }
}

fn firewall_protocol_to_proto(protocol: SecurityGroupProtocol) -> i32 {
    match protocol {
        SecurityGroupProtocol::Any => FirewallProtocol::Any as i32,
        SecurityGroupProtocol::Tcp => FirewallProtocol::Tcp as i32,
        SecurityGroupProtocol::Udp => FirewallProtocol::Udp as i32,
        SecurityGroupProtocol::Icmp => FirewallProtocol::Icmp as i32,
    }
}

pub async fn sync_host_network_isolation(env: &App, host_id: Uuid) -> Result<(), Error> {
    let host = hosts::require_by_id(env.pool(), host_id).await?;
    let attached_cluster = networks::list_attached(env.pool()).await?;
    let attached = attached_networks_for_host(&attached_cluster, host_id);
    sync_host_network_isolation_for_host(&host, &attached, &attached_cluster).await
}

async fn sync_host_network_isolation_for_host(
    host: &hosts::Host,
    attached_host_networks: &[(networks::Network, String)],
    attached_cluster: &[networks::AttachedNetwork],
) -> Result<(), Error> {
    let node_client = NodeClient::new(&host.address, host.port as u16);

    for (network, bridge_name) in attached_host_networks {
        let blocked_subnets =
            blocked_subnets_for_network(network, host.id, attached_host_networks, attached_cluster);
        let nat_exempt_subnets = nat_exempt_subnets_for_network(network, attached_cluster);

        node_client
            .sync_network_isolation(
                bridge_name,
                &network.subnet,
                &blocked_subnets,
                &nat_exempt_subnets,
            )
            .await
            .map_err(|e| {
                Error::UnprocessableEntity(format!(
                    "Failed to sync network isolation for host {} bridge {}: {e}",
                    host.name, bridge_name
                ))
            })?;
    }

    Ok(())
}

fn blocked_subnets_for_network(
    network: &networks::Network,
    host_id: Uuid,
    attached_host_networks: &[(networks::Network, String)],
    attached_cluster: &[networks::AttachedNetwork],
) -> Vec<String> {
    let Some(vpc_name) = network.vpc_name.as_deref() else {
        return Vec::new();
    };

    attached_host_networks
        .iter()
        .filter(|(other, _)| other.id != network.id && other.vpc_name.as_deref() != Some(vpc_name))
        .map(|(other, _)| other.subnet.clone())
        .chain(
            attached_cluster
                .iter()
                .filter(|other| {
                    other.host_id != host_id
                        && other.network.id != network.id
                        && other.network.vpc_name.as_deref() != Some(vpc_name)
                })
                .map(|other| other.network.subnet.clone()),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn nat_exempt_subnets_for_network(
    network: &networks::Network,
    attached_cluster: &[networks::AttachedNetwork],
) -> Vec<String> {
    let Some(vpc_name) = network.vpc_name.as_deref() else {
        return Vec::new();
    };
    if network.network_type.as_deref() != Some("isolated") {
        return Vec::new();
    }

    attached_cluster
        .iter()
        .filter(|other| {
            other.network.id != network.id && other.network.vpc_name.as_deref() == Some(vpc_name)
        })
        .map(|other| other.network.subnet.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub async fn sync_cluster_vpc_state(
    env: &App,
    network: &networks::Network,
    extra_host_ids: &[Uuid],
) -> Result<(), Error> {
    let Some(vpc_name) = network.vpc_name.as_deref() else {
        return Ok(());
    };

    let attached_cluster = networks::list_attached(env.pool()).await?;
    let host_ids = affected_host_ids_for_vpc(&attached_cluster, vpc_name, extra_host_ids);
    let attached_cluster_ref = &attached_cluster;

    try_join_all(host_ids.into_iter().map(|host_id| {
        let env = env.clone();
        async move { sync_host_vpc_state(&env, host_id, attached_cluster_ref).await }
    }))
    .await?;

    Ok(())
}

fn affected_host_ids_for_vpc(
    attached_cluster: &[networks::AttachedNetwork],
    vpc_name: &str,
    extra_host_ids: &[Uuid],
) -> Vec<Uuid> {
    let mut host_ids: BTreeSet<Uuid> = attached_cluster
        .iter()
        .filter(|attachment| attachment.network.vpc_name.as_deref() == Some(vpc_name))
        .map(|attachment| attachment.host_id)
        .collect();
    host_ids.extend(extra_host_ids.iter().copied());
    host_ids.into_iter().collect()
}

fn attached_networks_for_host(
    attached_cluster: &[networks::AttachedNetwork],
    host_id: Uuid,
) -> Vec<(networks::Network, String)> {
    attached_cluster
        .iter()
        .filter(|attachment| attachment.host_id == host_id)
        .map(|attachment| (attachment.network.clone(), attachment.bridge_name.clone()))
        .collect()
}

async fn sync_host_vpc_state(
    env: &App,
    host_id: Uuid,
    attached_cluster: &[networks::AttachedNetwork],
) -> Result<(), Error> {
    let host = hosts::require_by_id(env.pool(), host_id).await?;
    let attached_host_networks = attached_networks_for_host(attached_cluster, host_id);
    sync_host_network_isolation_for_host(&host, &attached_host_networks, attached_cluster).await?;
    sync_host_vpc_overlays_for_host(&host, attached_cluster).await?;
    Ok(())
}

pub async fn sync_host_vpc_overlays(env: &App, host_id: Uuid) -> Result<(), Error> {
    let host = hosts::require_by_id(env.pool(), host_id).await?;
    let attached = networks::list_attached(env.pool()).await?;
    sync_host_vpc_overlays_for_host(&host, &attached).await
}

async fn sync_host_vpc_overlays_for_host(
    host: &hosts::Host,
    attached: &[networks::AttachedNetwork],
) -> Result<(), Error> {
    let mut local_vpcs: BTreeMap<String, Vec<&networks::AttachedNetwork>> = BTreeMap::new();

    for attachment in attached {
        if attachment.host_id == host.id
            && let Some(vpc_name) = attachment.network.vpc_name.as_ref()
        {
            local_vpcs
                .entry(vpc_name.clone())
                .or_default()
                .push(attachment);
        }
    }

    let mut overlays = Vec::new();
    let mut resolved_underlay_ips = HashMap::new();

    for (vpc_name, _local_attachments) in local_vpcs {
        let participants = vpc_participants(attached, &vpc_name);
        if participants.len() <= 1 {
            continue;
        }

        let host_order = sorted_host_ids(&participants);
        if host_order.len() >= 255 {
            return Err(Error::UnprocessableEntity(format!(
                "VPC {vpc_name} exceeds the first-cut VXLAN host limit of 254 participants"
            )));
        }

        let local_host_index =
            host_order
                .iter()
                .position(|id| *id == host.id)
                .ok_or_else(|| {
                    Error::UnprocessableEntity(format!(
                        "Host {} is missing from VPC {vpc_name}",
                        host.id
                    ))
                })?;
        let local_underlay_ip =
            resolve_underlay_ip(&mut resolved_underlay_ips, &host.address, host.port as u16)
                .await?;
        let local_overlay_cidr = overlay_ip_cidr(&vpc_name, local_host_index)?;

        let mut peer_underlay_ips = BTreeSet::new();
        let mut routes = BTreeSet::new();

        for (remote_host_id, remote_attachments) in &participants {
            if *remote_host_id == host.id {
                continue;
            }
            let remote_attachment = remote_attachments.first().ok_or_else(|| {
                Error::UnprocessableEntity(format!(
                    "Missing attachment details for host {remote_host_id} in VPC {vpc_name}"
                ))
            })?;
            let remote_underlay_ip = resolve_underlay_ip(
                &mut resolved_underlay_ips,
                &remote_attachment.host_address,
                remote_attachment.host_port as u16,
            )
            .await?;
            peer_underlay_ips.insert(remote_underlay_ip);

            let remote_index = host_order
                .iter()
                .position(|id| id == remote_host_id)
                .ok_or_else(|| {
                    Error::UnprocessableEntity(format!(
                        "Host {remote_host_id} is missing from VPC ordering for {vpc_name}"
                    ))
                })?;
            let remote_overlay_ip = overlay_ip(&vpc_name, remote_index)?;
            for attachment in remote_attachments {
                routes.insert((attachment.network.subnet.clone(), remote_overlay_ip.clone()));
            }
        }

        overlays.push(VpcOverlayConfig {
            vpc_name: vpc_name.clone(),
            interface_name: overlay_interface_name(&vpc_name),
            vni: overlay_vni(&vpc_name),
            local_underlay_ip,
            local_overlay_cidr,
            peer_underlay_ips: peer_underlay_ips.into_iter().collect(),
            routes: routes
                .into_iter()
                .map(|(subnet, via_ip)| VpcOverlayRoute { subnet, via_ip })
                .collect(),
        });
    }

    let node_client = NodeClient::new(&host.address, host.port as u16);
    node_client.sync_vpc_overlays(&overlays).await.map_err(|e| {
        Error::UnprocessableEntity(format!(
            "Failed to sync VXLAN VPC overlays for host {}: {e}",
            host.name
        ))
    })
}

fn vpc_participants<'a>(
    attached: &'a [networks::AttachedNetwork],
    vpc_name: &str,
) -> BTreeMap<Uuid, Vec<&'a networks::AttachedNetwork>> {
    let mut participants: BTreeMap<Uuid, Vec<&'a networks::AttachedNetwork>> = BTreeMap::new();
    for attachment in attached {
        if attachment.network.vpc_name.as_deref() == Some(vpc_name) {
            participants
                .entry(attachment.host_id)
                .or_default()
                .push(attachment);
        }
    }
    participants
}

fn sorted_host_ids(participants: &BTreeMap<Uuid, Vec<&networks::AttachedNetwork>>) -> Vec<Uuid> {
    let mut host_ids: Vec<Uuid> = participants.keys().copied().collect();
    host_ids.sort_unstable_by_key(|host_id| host_id.as_u128());
    host_ids
}

fn overlay_seed(vpc_name: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_DNS,
        format!("qarax:vpc:{vpc_name}").as_bytes(),
    )
}

fn overlay_interface_name(vpc_name: &str) -> String {
    let seed = overlay_seed(vpc_name).simple().to_string();
    format!("qvx{}", &seed[..8])
}

fn overlay_vni(vpc_name: &str) -> u32 {
    let seed = overlay_seed(vpc_name);
    let bytes = seed.as_bytes();
    let vni = ((u32::from(bytes[0])) << 16) | ((u32::from(bytes[1])) << 8) | u32::from(bytes[2]);
    if vni == 0 { 1 } else { vni }
}

fn overlay_octets(vpc_name: &str) -> (u8, u8) {
    let bytes = overlay_seed(vpc_name).as_bytes().to_owned();
    (64 + (bytes[3] % 64), bytes[4])
}

fn overlay_ip(vpc_name: &str, host_index: usize) -> Result<String, Error> {
    if host_index >= 254 {
        return Err(Error::UnprocessableEntity(format!(
            "VPC {vpc_name} exceeds the first-cut VXLAN host limit of 254 participants"
        )));
    }
    let (octet2, octet3) = overlay_octets(vpc_name);
    Ok(format!("100.{octet2}.{octet3}.{}", host_index + 1))
}

fn overlay_ip_cidr(vpc_name: &str, host_index: usize) -> Result<String, Error> {
    Ok(format!("{}/24", overlay_ip(vpc_name, host_index)?))
}

async fn resolve_underlay_ip(
    cache: &mut HashMap<String, String>,
    address: &str,
    port: u16,
) -> Result<String, Error> {
    if let Some(ip) = cache.get(address) {
        return Ok(ip.clone());
    }

    let resolved = tokio::net::lookup_host((address, port))
        .await
        .map_err(|e| {
            Error::UnprocessableEntity(format!(
                "Failed to resolve host address {address} for VXLAN overlay: {e}"
            ))
        })?
        .find_map(|socket_addr| match socket_addr.ip() {
            std::net::IpAddr::V4(ip) => Some(ip.to_string()),
            std::net::IpAddr::V6(_) => None,
        })
        .ok_or_else(|| {
            Error::UnprocessableEntity(format!(
                "Host address {address} did not resolve to an IPv4 address for VXLAN overlay"
            ))
        })?;

    cache.insert(address.to_string(), resolved.clone());
    Ok(resolved)
}

async fn firewall_interfaces_for_vm(
    env: &App,
    vm_id: Uuid,
) -> Result<Vec<VmFirewallInterface>, Error> {
    let vm = vms::get(env.pool(), vm_id).await?;
    let Some(host_id) = vm.host_id else {
        return Ok(Vec::new());
    };

    if security_groups::list_by_vm(env.pool(), vm_id)
        .await?
        .is_empty()
    {
        return Ok(Vec::new());
    }

    let rules = security_groups::rule_set_for_vm(env.pool(), vm_id).await?;

    let proto_rules: Vec<VmFirewallRule> = rules
        .into_iter()
        .map(|rule| VmFirewallRule {
            direction: firewall_direction_to_proto(rule.direction),
            protocol: firewall_protocol_to_proto(rule.protocol),
            cidr: rule.cidr.unwrap_or_default(),
            port_start: rule.port_start,
            port_end: rule.port_end,
        })
        .collect();

    let nics = network_interfaces::list_by_vm(env.pool(), vm_id).await?;
    let mut interfaces = Vec::new();
    for nic in nics {
        let Some(network_id) = nic.network_id else {
            continue;
        };
        let Some(ip_address) = nic.ip_address.as_deref() else {
            continue;
        };

        let network = networks::get(env.pool(), network_id).await?;
        if network.network_type.as_deref() == Some("passt") {
            continue;
        }

        let Some(bridge_name) = networks::get_host_bridge(env.pool(), host_id, network_id).await?
        else {
            return Err(Error::UnprocessableEntity(format!(
                "Network {} is not attached to host {}. Run 'network attach-host' first.",
                network_id, host_id
            )));
        };

        interfaces.push(VmFirewallInterface {
            bridge_name,
            ip: ip_address
                .split('/')
                .next()
                .unwrap_or(ip_address)
                .to_string(),
            rules: proto_rules.clone(),
        });
    }

    Ok(interfaces)
}

pub async fn sync_vm_firewall_on_host(env: &App, vm_id: Uuid, host_id: Uuid) -> Result<(), Error> {
    let host = hosts::require_by_id(env.pool(), host_id).await?;
    let node_client = NodeClient::new(&host.address, host.port as u16);
    let interfaces = firewall_interfaces_for_vm(env, vm_id).await?;

    node_client
        .sync_vm_firewall(vm_id, &interfaces)
        .await
        .map_err(|e| {
            Error::UnprocessableEntity(format!(
                "Failed to sync VM firewall for {} on host {}: {e}",
                vm_id, host.name
            ))
        })
}

pub async fn sync_vm_firewall(env: &App, vm_id: Uuid) -> Result<(), Error> {
    let vm = vms::get(env.pool(), vm_id).await?;
    let Some(host_id) = vm.host_id else {
        return Ok(());
    };

    if !matches!(vm.status, VmStatus::Running | VmStatus::Shutdown) {
        return Ok(());
    }

    sync_vm_firewall_on_host(env, vm_id, host_id).await
}

pub async fn sync_security_group_members(env: &App, security_group_id: Uuid) -> Result<(), Error> {
    for vm_id in security_groups::list_vm_ids(env.pool(), security_group_id).await? {
        sync_vm_firewall(env, vm_id).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{blocked_subnets_for_network, nat_exempt_subnets_for_network};
    use crate::model::networks::{AttachedNetwork, Network, NetworkStatus};
    use uuid::Uuid;

    fn make_network(
        name: &str,
        subnet: &str,
        vpc_name: Option<&str>,
        network_type: Option<&str>,
    ) -> Network {
        Network {
            id: Uuid::new_v4(),
            name: name.to_string(),
            subnet: subnet.to_string(),
            gateway: None,
            dns: None,
            vpc_name: vpc_name.map(str::to_string),
            network_type: network_type.map(str::to_string),
            status: NetworkStatus::Active,
        }
    }

    fn make_attachment(host_id: Uuid, network: &Network) -> AttachedNetwork {
        AttachedNetwork {
            host_id,
            host_address: "host".to_string(),
            host_port: 50051,
            network: network.clone(),
            bridge_name: format!("br{}", &network.name),
        }
    }

    #[test]
    fn blocked_and_nat_exempt_subnets_follow_vpc_boundaries() {
        let host_a = Uuid::new_v4();
        let host_b = Uuid::new_v4();

        let local_vpc_a = make_network("a", "10.10.1.0/24", Some("vpc-a"), Some("isolated"));
        let local_other_vpc = make_network("b", "10.20.1.0/24", Some("vpc-b"), Some("isolated"));
        let local_same_vpc = make_network("c", "10.10.2.0/24", Some("vpc-a"), Some("isolated"));
        let remote_same_vpc = make_network("d", "10.10.3.0/24", Some("vpc-a"), Some("isolated"));
        let remote_other_vpc = make_network("e", "10.30.1.0/24", Some("vpc-c"), Some("isolated"));

        let attached_host_networks = vec![
            (local_vpc_a.clone(), "bra".to_string()),
            (local_other_vpc.clone(), "brb".to_string()),
            (local_same_vpc.clone(), "brc".to_string()),
        ];
        let attached_cluster = vec![
            make_attachment(host_a, &local_vpc_a),
            make_attachment(host_a, &local_other_vpc),
            make_attachment(host_a, &local_same_vpc),
            make_attachment(host_b, &remote_same_vpc),
            make_attachment(host_b, &remote_other_vpc),
        ];

        assert_eq!(
            blocked_subnets_for_network(
                &local_vpc_a,
                host_a,
                &attached_host_networks,
                &attached_cluster
            ),
            vec!["10.20.1.0/24".to_string(), "10.30.1.0/24".to_string()]
        );
        assert_eq!(
            nat_exempt_subnets_for_network(&local_vpc_a, &attached_cluster),
            vec!["10.10.2.0/24".to_string(), "10.10.3.0/24".to_string()]
        );
    }

    #[test]
    fn affected_hosts_are_scoped_to_the_changed_vpc() {
        let host_a = Uuid::new_v4();
        let host_b = Uuid::new_v4();
        let host_c = Uuid::new_v4();

        let vpc_a_local = make_network("a", "10.10.1.0/24", Some("vpc-a"), Some("isolated"));
        let vpc_a_remote = make_network("b", "10.10.2.0/24", Some("vpc-a"), Some("isolated"));
        let vpc_b_remote = make_network("c", "10.20.1.0/24", Some("vpc-b"), Some("isolated"));
        let attached_cluster = vec![
            make_attachment(host_a, &vpc_a_local),
            make_attachment(host_b, &vpc_a_remote),
            make_attachment(host_c, &vpc_b_remote),
        ];
        let mut expected = vec![host_a, host_b, host_c];
        expected.sort_unstable();

        assert_eq!(
            super::affected_host_ids_for_vpc(&attached_cluster, "vpc-a", &[host_c]),
            expected
        );
    }
}
