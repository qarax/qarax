use uuid::Uuid;

use crate::{
    App,
    errors::Error,
    grpc_client::{
        NodeClient,
        node::{FirewallDirection, FirewallProtocol, VmFirewallInterface, VmFirewallRule},
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
    let attached = networks::list_for_host(env.pool(), host_id).await?;
    let node_client = NodeClient::new(&host.address, host.port as u16);

    for (network, bridge_name) in &attached {
        let blocked_subnets = match network.vpc_name.as_deref() {
            Some(vpc_name) => attached
                .iter()
                .filter(|(other, _)| {
                    other.id != network.id && other.vpc_name.as_deref() != Some(vpc_name)
                })
                .map(|(other, _)| other.subnet.clone())
                .collect(),
            None => Vec::new(),
        };

        node_client
            .sync_network_isolation(bridge_name, &blocked_subnets)
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
