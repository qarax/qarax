use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    api::{self, models::NewNetwork},
    client::Client,
};

use super::{OutputFormat, print_output, resolve_host_id, resolve_network_id};

#[derive(Args)]
pub struct NetworkArgs {
    #[command(subcommand)]
    command: NetworkCommand,
}

#[derive(Subcommand)]
enum NetworkCommand {
    /// List all networks
    List,
    /// Get details of a network
    Get {
        /// Network name or ID
        network: String,
    },
    /// Create a network
    Create {
        /// Network name
        #[arg(long)]
        name: String,
        /// Subnet in CIDR notation (e.g. 10.0.0.0/24)
        #[arg(long)]
        subnet: String,
        /// Gateway IP address (e.g. 10.0.0.1)
        #[arg(long)]
        gateway: Option<String>,
        /// DNS server IP address
        #[arg(long)]
        dns: Option<String>,
        /// Optional VPC name. Networks with the same VPC name on one host can route between subnets.
        #[arg(long)]
        vpc: Option<String>,
        /// Network type (bridge or vlan)
        #[arg(long, value_name = "TYPE", default_value = "bridge")]
        network_type: String,
    },
    /// Delete a network
    Delete {
        /// Network name or ID
        network: String,
    },
    /// Attach a network to a host (creates bridge + DHCP + NAT on the host)
    AttachHost {
        /// Network name or ID
        #[arg(long)]
        network: String,
        /// Host name or ID
        #[arg(long)]
        host: String,
        /// Bridge device name on the host (max 15 chars, e.g. qbr0)
        #[arg(long)]
        bridge_name: String,
        /// Parent NIC to bridge (e.g. eth0). If set, bridges the NIC instead of
        /// creating an isolated bridge — skips NAT. VMs get IPs from
        /// the upstream network.
        #[arg(long)]
        parent_interface: Option<String>,
    },
    /// Detach a network from a host
    DetachHost {
        /// Network name or ID
        #[arg(long)]
        network: String,
        /// Host name or ID
        #[arg(long)]
        host: String,
    },
    /// List IP allocations for a network
    ListIps {
        /// Network name or ID
        network: String,
    },
}

#[derive(Tabled)]
struct NetworkRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Subnet")]
    subnet: String,
    #[tabled(rename = "Gateway")]
    gateway: String,
    #[tabled(rename = "VPC")]
    vpc_name: String,
    #[tabled(rename = "Type")]
    network_type: String,
    #[tabled(rename = "Status")]
    status: String,
}

#[derive(Tabled)]
struct IpRow {
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "VM")]
    vm_id: String,
    #[tabled(rename = "Allocated")]
    allocated_at: String,
}

pub async fn run(args: NetworkArgs, client: &Client, output: OutputFormat) -> anyhow::Result<()> {
    match args.command {
        NetworkCommand::List => {
            let networks = api::networks::list(client, None).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&networks, output)?;
            } else {
                let rows: Vec<NetworkRow> = networks
                    .iter()
                    .map(|n| NetworkRow {
                        id: n.id.to_string(),
                        name: n.name.clone(),
                        subnet: n.subnet.clone(),
                        gateway: n.gateway.clone().unwrap_or_else(|| "-".to_string()),
                        vpc_name: n.vpc_name.clone().unwrap_or_else(|| "-".to_string()),
                        network_type: n.network_type.clone().unwrap_or_else(|| "-".to_string()),
                        status: n.status.clone(),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }

        NetworkCommand::Get { network } => {
            let id = resolve_network_id(client, &network).await?;
            let net = api::networks::get(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&net, output)?;
            } else {
                println!("ID:      {}", net.id);
                println!("Name:    {}", net.name);
                println!("Subnet:  {}", net.subnet);
                println!(
                    "Gateway: {}",
                    net.gateway.unwrap_or_else(|| "-".to_string())
                );
                println!("DNS:     {}", net.dns.unwrap_or_else(|| "-".to_string()));
                println!(
                    "VPC:     {}",
                    net.vpc_name.unwrap_or_else(|| "-".to_string())
                );
                println!(
                    "Type:    {}",
                    net.network_type.unwrap_or_else(|| "-".to_string())
                );
                println!("Status:  {}", net.status);
            }
        }

        NetworkCommand::Create {
            name,
            subnet,
            gateway,
            dns,
            vpc,
            network_type,
        } => {
            let new_net = NewNetwork {
                name,
                subnet,
                gateway,
                dns,
                vpc_name: vpc,
                network_type: Some(network_type),
            };
            let id = api::networks::create(client, &new_net).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "network_id": id }), output)?;
            } else {
                println!("Created network: {id}");
            }
        }

        NetworkCommand::Delete { network } => {
            let id = resolve_network_id(client, &network).await?;
            api::networks::delete(client, id).await?;
            println!("Deleted network: {network}");
        }

        NetworkCommand::AttachHost {
            network,
            host,
            bridge_name,
            parent_interface,
        } => {
            let network_id = resolve_network_id(client, &network).await?;
            let host_id = resolve_host_id(client, &host).await?;
            api::networks::attach_host(
                client,
                network_id,
                host_id,
                &bridge_name,
                parent_interface.as_deref(),
            )
            .await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(
                    &serde_json::json!({ "network_id": network_id, "host_id": host_id }),
                    output,
                )?;
            } else {
                println!("Attached host {host} to network {network} (bridge: {bridge_name})");
            }
        }

        NetworkCommand::DetachHost { network, host } => {
            let network_id = resolve_network_id(client, &network).await?;
            let host_id = resolve_host_id(client, &host).await?;
            api::networks::detach_host(client, network_id, host_id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(
                    &serde_json::json!({ "network_id": network_id, "host_id": host_id }),
                    output,
                )?;
            } else {
                println!("Detached host {host} from network {network}");
            }
        }

        NetworkCommand::ListIps { network } => {
            let id = resolve_network_id(client, &network).await?;
            let ips = api::networks::list_ips(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&ips, output)?;
            } else {
                let rows: Vec<IpRow> = ips
                    .iter()
                    .map(|a| IpRow {
                        ip: a.ip_address.clone(),
                        vm_id: a
                            .vm_id
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        allocated_at: a.allocated_at.clone(),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
    }

    Ok(())
}
