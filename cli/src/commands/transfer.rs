use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};
use uuid::Uuid;

use crate::{
    api::{self, models::NewTransfer},
    client::Client,
};

use super::{format_bytes, print_json, resolve_pool_id};

#[derive(Args)]
pub struct TransferArgs {
    #[command(subcommand)]
    command: TransferCommand,
}

#[derive(Subcommand)]
enum TransferCommand {
    /// List all transfers in a storage pool
    List {
        /// Storage pool name or ID
        #[arg(long)]
        pool: String,
    },
    /// Get details of a specific transfer
    Get {
        /// Storage pool name or ID
        #[arg(long)]
        pool: String,
        /// Transfer ID
        id: Uuid,
    },
    /// Start a new transfer into a storage pool
    Create {
        /// Storage pool name or ID
        #[arg(long)]
        pool: String,
        /// Transfer name
        #[arg(long)]
        name: String,
        /// Source URL (http/https) or local path
        #[arg(long)]
        source: String,
        /// Object type (disk, kernel, initrd, iso, snapshot, oci_image)
        #[arg(long, value_name = "TYPE")]
        object_type: String,
    },
}

#[derive(Tabled)]
struct TransferRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    transfer_type: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Source")]
    source: String,
    #[tabled(rename = "Transferred")]
    transferred: String,
}

pub async fn run(args: TransferArgs, client: &Client, json: bool) -> anyhow::Result<()> {
    match args.command {
        TransferCommand::List { pool } => {
            let pool_id = resolve_pool_id(client, &pool).await?;
            let transfers = api::transfers::list(client, pool_id).await?;
            if json {
                print_json(&transfers)?;
            } else {
                let rows: Vec<TransferRow> = transfers
                    .iter()
                    .map(|t| TransferRow {
                        id: t.id.to_string(),
                        name: t.name.clone(),
                        transfer_type: t.transfer_type.clone(),
                        status: t.status.clone(),
                        source: t.source.clone(),
                        transferred: format_bytes(t.transferred_bytes),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }

        TransferCommand::Get { pool, id } => {
            let pool_id = resolve_pool_id(client, &pool).await?;
            let transfer = api::transfers::get(client, pool_id, id).await?;
            if json {
                print_json(&transfer)?;
            } else {
                println!("ID:          {}", transfer.id);
                println!("Name:        {}", transfer.name);
                println!("Type:        {}", transfer.transfer_type);
                println!("Status:      {}", transfer.status);
                println!("Source:      {}", transfer.source);
                println!("Pool:        {}", transfer.storage_pool_id);
                println!("Transferred: {}", format_bytes(transfer.transferred_bytes));
                if let Some(total) = transfer.total_bytes {
                    println!("Total:       {}", format_bytes(total));
                }
                if let Some(err) = &transfer.error_message {
                    println!("Error:       {err}");
                }
            }
        }

        TransferCommand::Create {
            pool,
            name,
            source,
            object_type,
        } => {
            let pool_id = resolve_pool_id(client, &pool).await?;
            let new_transfer = NewTransfer {
                name,
                source,
                object_type,
            };
            let transfer = api::transfers::create(client, pool_id, &new_transfer).await?;
            if json {
                print_json(&transfer)?;
            } else {
                println!("Transfer started: {}", transfer.id);
                println!("Status: {}", transfer.status);
            }
        }
    }

    Ok(())
}
