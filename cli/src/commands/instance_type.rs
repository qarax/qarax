use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    api::{self, models::NewInstanceType},
    client::Client,
};

use super::{
    OutputFormat, build_accelerator_config, format_bytes, print_output, resolve_instance_type_id,
};

#[derive(Args)]
pub struct InstanceTypeArgs {
    #[command(subcommand)]
    command: InstanceTypeCommand,
}

#[derive(Subcommand)]
enum InstanceTypeCommand {
    /// List all instance types
    List,
    /// Get details of an instance type
    Get {
        /// Instance type name or ID
        instance_type: String,
    },
    /// Create a new instance type
    Create {
        /// Instance type name
        #[arg(long)]
        name: String,
        /// Number of vCPUs at boot
        #[arg(long)]
        vcpus: i32,
        /// Maximum vCPUs (defaults to --vcpus)
        #[arg(long)]
        max_vcpus: Option<i32>,
        /// Memory size in bytes
        #[arg(long)]
        memory: i64,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Target architecture (e.g. x86_64, aarch64, riscv64)
        #[arg(long)]
        architecture: Option<String>,
        /// Number of GPUs to request
        #[arg(long)]
        gpu_count: Option<i32>,
        /// Filter GPUs by vendor (e.g. "nvidia")
        #[arg(long, requires = "gpu_count")]
        gpu_vendor: Option<String>,
        /// Filter GPUs by model (e.g. "NVIDIA A100")
        #[arg(long, requires = "gpu_count")]
        gpu_model: Option<String>,
        /// Minimum GPU VRAM in bytes
        #[arg(long, requires = "gpu_count")]
        min_vram: Option<i64>,
        /// Pin VMs of this type to the NUMA node local to their allocated GPU (default: true when gpu_count > 0)
        #[arg(long, requires = "gpu_count")]
        prefer_local_numa: Option<bool>,
    },
    /// Delete an instance type
    Delete {
        /// Instance type name or ID
        instance_type: String,
    },
}

#[derive(Tabled)]
struct InstanceTypeRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "vCPUs")]
    vcpus: String,
    #[tabled(rename = "Memory")]
    memory: String,
}

pub async fn run(
    args: InstanceTypeArgs,
    client: &Client,
    output: OutputFormat,
) -> anyhow::Result<()> {
    match args.command {
        InstanceTypeCommand::List => {
            let instance_types = api::instance_types::list(client, None).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&instance_types, output)?;
            } else {
                let rows: Vec<InstanceTypeRow> = instance_types
                    .iter()
                    .map(|instance_type| InstanceTypeRow {
                        id: instance_type.id.to_string(),
                        name: instance_type.name.clone(),
                        vcpus: format!("{}/{}", instance_type.boot_vcpus, instance_type.max_vcpus),
                        memory: format_bytes(instance_type.memory_size),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
        InstanceTypeCommand::Get { instance_type } => {
            let id = resolve_instance_type_id(client, &instance_type).await?;
            let instance_type = api::instance_types::get(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&instance_type, output)?;
            } else {
                println!("ID:          {}", instance_type.id);
                println!("Name:        {}", instance_type.name);
                if let Some(description) = &instance_type.description {
                    println!("Description: {description}");
                }
                println!(
                    "vCPUs:       {}/{}",
                    instance_type.boot_vcpus, instance_type.max_vcpus
                );
                println!("Memory:      {}", format_bytes(instance_type.memory_size));
            }
        }
        InstanceTypeCommand::Create {
            name,
            vcpus,
            max_vcpus,
            memory,
            description,
            architecture,
            gpu_count,
            gpu_vendor,
            gpu_model,
            min_vram,
            prefer_local_numa,
        } => {
            let accelerator_config =
                build_accelerator_config(gpu_count, &gpu_vendor, &gpu_model, min_vram);
            // Embed prefer_local_numa into accelerator_config if provided
            let accelerator_config = match (accelerator_config, prefer_local_numa) {
                (Some(mut ac), Some(pln)) => {
                    if let serde_json::Value::Object(ref mut map) = ac {
                        map.insert(
                            "prefer_local_numa".to_string(),
                            serde_json::Value::Bool(pln),
                        );
                    }
                    Some(ac)
                }
                (ac, _) => ac,
            };
            let new_instance_type = NewInstanceType {
                name,
                description,
                architecture,
                boot_vcpus: vcpus,
                max_vcpus: max_vcpus.unwrap_or(vcpus),
                memory_size: memory,
                accelerator_config,
                numa_config: None,
            };
            let id = api::instance_types::create(client, &new_instance_type).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "instance_type_id": id }), output)?;
            } else {
                println!("Created instance type: {id}");
            }
        }
        InstanceTypeCommand::Delete { instance_type } => {
            let id = resolve_instance_type_id(client, &instance_type).await?;
            api::instance_types::delete(client, id).await?;
            println!("Deleted instance type: {id}");
        }
    }

    Ok(())
}
