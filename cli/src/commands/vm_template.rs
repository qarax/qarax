use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    api::{
        self,
        models::{CreateVmTemplateFromVmRequest, NewVmTemplate},
    },
    client::Client,
};

use super::{
    OutputFormat, format_bytes, print_output, resolve_boot_source_id, resolve_network_id,
    resolve_object_id, resolve_vm_id, resolve_vm_template_id,
};

#[derive(Args)]
pub struct VmTemplateArgs {
    #[command(subcommand)]
    command: VmTemplateCommand,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum VmTemplateCommand {
    /// List all VM templates
    List,
    /// Get details of a VM template
    Get {
        /// VM template name or ID
        vm_template: String,
    },
    /// Create a new VM template
    Create {
        /// VM template name
        #[arg(long)]
        name: String,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Create the template from an existing VM name or ID
        #[arg(long)]
        from_vm: Option<String>,
        /// Hypervisor for VMs created from this template
        #[arg(long)]
        hypervisor: Option<String>,
        /// Number of vCPUs at boot
        #[arg(long)]
        vcpus: Option<i32>,
        /// Maximum vCPUs
        #[arg(long)]
        max_vcpus: Option<i32>,
        /// Memory size in bytes
        #[arg(long)]
        memory: Option<i64>,
        /// Boot source name or ID
        #[arg(long)]
        boot_source: Option<String>,
        /// Root disk storage object name or ID
        #[arg(long)]
        root_disk: Option<String>,
        /// Boot mode: kernel or firmware
        #[arg(long)]
        boot_mode: Option<String>,
        /// OCI image reference
        #[arg(long)]
        image_ref: Option<String>,
        /// Network name or ID to attach the VM to
        #[arg(long)]
        network: Option<String>,
    },
    /// Delete a VM template
    Delete {
        /// VM template name or ID
        vm_template: String,
    },
}

#[derive(Tabled)]
struct VmTemplateRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Hypervisor")]
    hypervisor: String,
    #[tabled(rename = "vCPUs")]
    vcpus: String,
    #[tabled(rename = "Memory")]
    memory: String,
}

pub async fn run(
    args: VmTemplateArgs,
    client: &Client,
    output: OutputFormat,
) -> anyhow::Result<()> {
    match args.command {
        VmTemplateCommand::List => {
            let templates = api::vm_templates::list(client, None).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&templates, output)?;
            } else {
                let rows: Vec<VmTemplateRow> = templates
                    .iter()
                    .map(|template| VmTemplateRow {
                        id: template.id.to_string(),
                        name: template.name.clone(),
                        hypervisor: template
                            .hypervisor
                            .clone()
                            .unwrap_or_else(|| "-".to_string()),
                        vcpus: match (template.boot_vcpus, template.max_vcpus) {
                            (Some(boot), Some(max)) => format!("{boot}/{max}"),
                            (Some(boot), None) => boot.to_string(),
                            _ => "-".to_string(),
                        },
                        memory: template
                            .memory_size
                            .map(format_bytes)
                            .unwrap_or_else(|| "-".to_string()),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
        VmTemplateCommand::Get { vm_template } => {
            let id = resolve_vm_template_id(client, &vm_template).await?;
            let template = api::vm_templates::get(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&template, output)?;
            } else {
                println!("ID:          {}", template.id);
                println!("Name:        {}", template.name);
                if let Some(description) = &template.description {
                    println!("Description: {description}");
                }
                if let Some(hypervisor) = &template.hypervisor {
                    println!("Hypervisor:  {hypervisor}");
                }
                if let Some(boot_vcpus) = template.boot_vcpus {
                    let max_vcpus = template.max_vcpus.unwrap_or(boot_vcpus);
                    println!("vCPUs:       {boot_vcpus}/{max_vcpus}");
                }
                if let Some(memory_size) = template.memory_size {
                    println!("Memory:      {}", format_bytes(memory_size));
                }
                if let Some(boot_source_id) = template.boot_source_id {
                    println!("Boot source: {boot_source_id}");
                }
                if let Some(root_disk_object_id) = template.root_disk_object_id {
                    println!("Root disk:   {root_disk_object_id}");
                }
                if let Some(boot_mode) = &template.boot_mode {
                    println!("Boot mode:   {boot_mode}");
                }
                if let Some(image_ref) = &template.image_ref {
                    println!("Image:       {image_ref}");
                }
                if let Some(network_id) = template.network_id {
                    println!("Network:     {network_id}");
                }
            }
        }
        VmTemplateCommand::Create {
            name,
            description,
            from_vm,
            hypervisor,
            vcpus,
            max_vcpus,
            memory,
            boot_source,
            root_disk,
            boot_mode,
            image_ref,
            network,
        } => {
            let boot_source_id = match boot_source {
                Some(ref source) => Some(resolve_boot_source_id(client, source).await?),
                None => None,
            };
            let network_id = match network {
                Some(ref network) => Some(resolve_network_id(client, network).await?),
                None => None,
            };
            let root_disk_object_id = match root_disk {
                Some(ref object) => Some(resolve_object_id(client, object).await?),
                None => None,
            };
            let id = if let Some(from_vm) = from_vm {
                if hypervisor.is_some()
                    || vcpus.is_some()
                    || max_vcpus.is_some()
                    || memory.is_some()
                    || boot_source_id.is_some()
                    || root_disk_object_id.is_some()
                    || boot_mode.is_some()
                    || image_ref.is_some()
                    || network_id.is_some()
                {
                    anyhow::bail!(
                        "--from-vm cannot be combined with manual template field flags; use only --name and optional --description"
                    );
                }
                let vm_id = resolve_vm_id(client, &from_vm).await?;
                let request = CreateVmTemplateFromVmRequest { name, description };
                api::vm_templates::create_from_vm(client, vm_id, &request).await?
            } else {
                let new_vm_template = NewVmTemplate {
                    name,
                    description,
                    hypervisor: hypervisor.or_else(|| Some("cloud_hv".to_string())),
                    boot_vcpus: vcpus,
                    max_vcpus,
                    memory_size: memory,
                    boot_source_id,
                    root_disk_object_id,
                    boot_mode,
                    image_ref,
                    network_id,
                    config: None,
                };
                api::vm_templates::create(client, &new_vm_template).await?
            };
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "vm_template_id": id }), output)?;
            } else {
                println!("Created VM template: {id}");
            }
        }
        VmTemplateCommand::Delete { vm_template } => {
            let id = resolve_vm_template_id(client, &vm_template).await?;
            api::vm_templates::delete(client, id).await?;
            println!("Deleted VM template: {id}");
        }
    }

    Ok(())
}
