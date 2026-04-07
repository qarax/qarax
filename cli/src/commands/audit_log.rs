use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};
use uuid::Uuid;

use crate::{api, client::Client};

use super::{OutputFormat, print_output};

#[derive(Args)]
pub struct AuditLogArgs {
    #[command(subcommand)]
    command: AuditLogCommand,
}

#[derive(Subcommand)]
enum AuditLogCommand {
    /// List audit log entries
    List {
        /// Filter by resource type (vm, host, storage_pool, network, ...)
        #[arg(long)]
        resource_type: Option<String>,
        /// Filter by resource UUID
        #[arg(long)]
        resource_id: Option<Uuid>,
        /// Filter by action (create, start, stop, delete, ...)
        #[arg(long)]
        action: Option<String>,
        /// Maximum number of entries to return (default: 100)
        #[arg(long)]
        limit: Option<i64>,
    },
    /// Get a single audit log entry by ID
    Get {
        /// Audit log entry ID
        id: Uuid,
    },
}

#[derive(Tabled)]
struct AuditLogRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "ACTION")]
    action: String,
    #[tabled(rename = "RESOURCE_TYPE")]
    resource_type: String,
    #[tabled(rename = "RESOURCE_ID")]
    resource_id: String,
    #[tabled(rename = "CREATED_AT")]
    created_at: String,
    #[tabled(rename = "NAME")]
    name: String,
}

pub async fn run(args: AuditLogArgs, client: &Client, output: OutputFormat) -> anyhow::Result<()> {
    match args.command {
        AuditLogCommand::List {
            resource_type,
            resource_id,
            action,
            limit,
        } => {
            let logs = api::audit_log::list(
                client,
                resource_type.as_deref(),
                resource_id,
                action.as_deref(),
                limit,
            )
            .await?;

            if !matches!(output, OutputFormat::Table) {
                print_output(&logs, output)?;
            } else {
                let rows: Vec<AuditLogRow> = logs
                    .iter()
                    .map(|log| AuditLogRow {
                        id: log.id.to_string(),
                        action: log.action.clone(),
                        resource_type: log.resource_type.clone(),
                        resource_id: log.resource_id.to_string(),
                        created_at: log
                            .created_at
                            .get(..19)
                            .unwrap_or(&log.created_at)
                            .to_string(),
                        name: log.resource_name.clone().unwrap_or_else(|| "-".to_string()),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
        AuditLogCommand::Get { id } => {
            let log = api::audit_log::get(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&log, output)?;
            } else {
                println!("ID:            {}", log.id);
                println!("Action:        {}", log.action);
                println!("Resource Type: {}", log.resource_type);
                println!("Resource ID:   {}", log.resource_id);
                if let Some(name) = &log.resource_name {
                    println!("Resource Name: {name}");
                }
                println!("Created At:    {}", log.created_at);
                if let Some(meta) = &log.metadata {
                    println!(
                        "Metadata:      {}",
                        serde_json::to_string_pretty(meta).unwrap_or_default()
                    );
                }
            }
        }
    }

    Ok(())
}
