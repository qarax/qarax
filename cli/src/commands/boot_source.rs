use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    api::{self, models::NewBootSource},
    client::Client,
};

use super::{OutputFormat, print_output, resolve_boot_source_id, resolve_object_id};

#[derive(Args)]
pub struct BootSourceArgs {
    #[command(subcommand)]
    command: BootSourceCommand,
}

#[derive(Subcommand)]
enum BootSourceCommand {
    /// List all boot sources
    List,
    /// Get details of a boot source
    Get {
        /// Boot source name or ID
        boot_source: String,
    },
    /// Create a new boot source
    Create {
        /// Boot source name
        #[arg(long)]
        name: String,
        /// Kernel storage object name or ID
        #[arg(long)]
        kernel: String,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Kernel command-line parameters
        #[arg(long)]
        params: Option<String>,
        /// Initrd storage object name or ID
        #[arg(long)]
        initrd: Option<String>,
    },
    /// Delete a boot source
    Delete {
        /// Boot source name or ID
        boot_source: String,
    },
}

#[derive(Tabled)]
struct BootSourceRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Kernel ID")]
    kernel_id: String,
    #[tabled(rename = "Params")]
    params: String,
    #[tabled(rename = "Initrd ID")]
    initrd_id: String,
}

pub async fn run(
    args: BootSourceArgs,
    client: &Client,
    output: OutputFormat,
) -> anyhow::Result<()> {
    match args.command {
        BootSourceCommand::List => {
            let sources = api::boot_sources::list(client, None).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&sources, output)?;
            } else {
                let rows: Vec<BootSourceRow> = sources
                    .iter()
                    .map(|bs| BootSourceRow {
                        id: bs.id.to_string(),
                        name: bs.name.clone(),
                        kernel_id: bs.kernel_image_id.to_string(),
                        params: bs.kernel_params.clone().unwrap_or_else(|| "-".to_string()),
                        initrd_id: bs
                            .initrd_image_id
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }

        BootSourceCommand::Get { boot_source } => {
            let id = resolve_boot_source_id(client, &boot_source).await?;
            let bs = api::boot_sources::get(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&bs, output)?;
            } else {
                println!("ID:          {}", bs.id);
                println!("Name:        {}", bs.name);
                if let Some(desc) = &bs.description {
                    println!("Description: {desc}");
                }
                println!("Kernel:      {}", bs.kernel_image_id);
                if let Some(p) = &bs.kernel_params {
                    println!("Params:      {p}");
                }
                if let Some(i) = bs.initrd_image_id {
                    println!("Initrd:      {i}");
                }
            }
        }

        BootSourceCommand::Create {
            name,
            kernel,
            description,
            params,
            initrd,
        } => {
            let kernel_image_id = resolve_object_id(client, &kernel).await?;
            let initrd_image_id = match initrd {
                Some(ref s) => Some(resolve_object_id(client, s).await?),
                None => None,
            };
            let new_bs = NewBootSource {
                name,
                description,
                kernel_image_id,
                kernel_params: params,
                initrd_image_id,
            };
            let id = api::boot_sources::create(client, &new_bs).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "boot_source_id": id }), output)?;
            } else {
                println!("Created boot source: {id}");
            }
        }

        BootSourceCommand::Delete { boot_source } => {
            let id = resolve_boot_source_id(client, &boot_source).await?;
            api::boot_sources::delete(client, id).await?;
            println!("Deleted boot source: {id}");
        }
    }

    Ok(())
}
