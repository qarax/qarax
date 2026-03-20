use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    api::{
        self,
        models::{DeployHostRequest, NewHost},
    },
    client::Client,
};

use super::{OutputFormat, format_bytes, print_output, resolve_host_id};

#[derive(Args)]
pub struct HostArgs {
    #[command(subcommand)]
    command: HostCommand,
}

#[derive(Subcommand)]
enum HostCommand {
    /// List all hosts
    List,
    /// Add a new host
    Add {
        /// Host name
        #[arg(long)]
        name: String,
        /// Host address (IP or hostname)
        #[arg(long)]
        address: String,
        /// SSH port
        #[arg(long, default_value = "22")]
        port: i32,
        /// SSH user
        #[arg(long)]
        user: String,
        /// SSH password
        #[arg(long)]
        password: String,
    },
    /// Deploy a bootc image to a host
    Deploy {
        /// Host name or ID
        host: String,
        /// Bootc image reference to deploy
        #[arg(long)]
        image: String,
        /// SSH port override
        #[arg(long)]
        ssh_port: Option<u16>,
        /// SSH user override
        #[arg(long)]
        ssh_user: Option<String>,
        /// SSH password override
        #[arg(long)]
        ssh_password: Option<String>,
        /// Path to SSH private key
        #[arg(long)]
        ssh_key: Option<String>,
        /// Install bootc before switching (default: true)
        #[arg(long)]
        install_bootc: Option<bool>,
        /// Reboot after bootc switch (default: true)
        #[arg(long)]
        reboot: Option<bool>,
    },
    /// Initialize a host (connect via gRPC, mark as UP)
    Init {
        /// Host name or ID
        host: String,
    },
    /// List GPUs on a host
    Gpus {
        /// Host name or ID
        host: String,
    },
}

#[derive(Tabled)]
struct HostRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Address")]
    address: String,
    #[tabled(rename = "Port")]
    port: i32,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "User")]
    host_user: String,
    #[tabled(rename = "CH Version")]
    ch_version: String,
    #[tabled(rename = "CPUs")]
    total_cpus: String,
    #[tabled(rename = "Memory")]
    memory: String,
    #[tabled(rename = "Load")]
    load: String,
}

#[derive(Tabled)]
struct GpuRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "PCI Address")]
    pci_address: String,
    #[tabled(rename = "Vendor")]
    vendor: String,
    #[tabled(rename = "Model")]
    model: String,
    #[tabled(rename = "VRAM")]
    vram: String,
    #[tabled(rename = "IOMMU Group")]
    iommu_group: i32,
    #[tabled(rename = "VM")]
    vm_id: String,
}

pub async fn run(args: HostArgs, client: &Client, output: OutputFormat) -> anyhow::Result<()> {
    match args.command {
        HostCommand::List => {
            let hosts = api::hosts::list(client, None).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&hosts, output)?;
            } else {
                let rows: Vec<HostRow> = hosts
                    .iter()
                    .map(|h| HostRow {
                        id: h.id.to_string(),
                        name: h.name.clone(),
                        address: h.address.clone(),
                        port: h.port,
                        status: h.status.clone(),
                        host_user: h.host_user.clone(),
                        ch_version: h
                            .cloud_hypervisor_version
                            .clone()
                            .unwrap_or_else(|| "-".to_string()),
                        total_cpus: h
                            .total_cpus
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        memory: h
                            .total_memory_bytes
                            .map(format_bytes)
                            .unwrap_or_else(|| "-".to_string()),
                        load: h
                            .load_average
                            .map(|l| format!("{:.2}", l))
                            .unwrap_or_else(|| "-".to_string()),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }

        HostCommand::Add {
            name,
            address,
            port,
            user,
            password,
        } => {
            let new_host = NewHost {
                name,
                address,
                port,
                host_user: user,
                password,
            };
            let id = api::hosts::add(client, &new_host).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "host_id": id }), output)?;
            } else {
                println!("Added host: {}", new_host.name);
            }
        }

        HostCommand::Deploy {
            host,
            image,
            ssh_port,
            ssh_user,
            ssh_password,
            ssh_key,
            install_bootc,
            reboot,
        } => {
            let id = resolve_host_id(client, &host).await?;
            let req = DeployHostRequest {
                image,
                ssh_port,
                ssh_user,
                ssh_password,
                ssh_private_key_path: ssh_key,
                install_bootc,
                reboot,
            };
            api::hosts::deploy(client, id, &req).await?;
            println!("Host deployment started: {host}");
        }

        HostCommand::Init { host } => {
            let id = resolve_host_id(client, &host).await?;
            let host = api::hosts::init(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&host, output)?;
            } else {
                println!("Initialized host: {} ({})", host.name, host.status);
                if let Some(ch) = &host.cloud_hypervisor_version {
                    println!("  Cloud Hypervisor: {ch}");
                }
                if let Some(k) = &host.kernel_version {
                    println!("  Kernel:           {k}");
                }
            }
        }

        HostCommand::Gpus { host } => {
            let id = resolve_host_id(client, &host).await?;
            let gpus = api::hosts::list_gpus(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&gpus, output)?;
            } else {
                let rows: Vec<GpuRow> = gpus
                    .iter()
                    .map(|g| GpuRow {
                        id: g.id.to_string(),
                        pci_address: g.pci_address.clone(),
                        vendor: g.vendor.clone().unwrap_or_else(|| "-".to_string()),
                        model: g.model.clone().unwrap_or_else(|| "-".to_string()),
                        vram: g
                            .vram_bytes
                            .map(format_bytes)
                            .unwrap_or_else(|| "-".to_string()),
                        iommu_group: g.iommu_group,
                        vm_id: g
                            .vm_id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
    }

    Ok(())
}
