use clap::{Args, Subcommand, ValueEnum};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    api::{self, models::CreateBackupRequest},
    client::Client,
};

use super::{OutputFormat, print_output, resolve_backup_id, resolve_pool_id, resolve_vm_id};

#[derive(Args)]
pub struct BackupArgs {
    #[command(subcommand)]
    command: BackupCommand,
}

#[derive(Subcommand)]
enum BackupCommand {
    /// List backups
    List {
        /// Optional exact backup name filter
        #[arg(long)]
        name: Option<String>,
        /// Optional backup type filter
        #[arg(long = "type", value_enum)]
        backup_type: Option<BackupKind>,
    },
    /// Get details of a backup
    Get {
        /// Backup name or ID
        backup: String,
    },
    /// Create a backup
    Create {
        #[command(subcommand)]
        command: CreateBackupCommand,
    },
    /// Restore from a backup
    Restore {
        /// Backup name or ID
        backup: String,
    },
}

#[derive(Subcommand)]
enum CreateBackupCommand {
    /// Create a VM backup (wraps snapshot creation)
    Vm {
        /// VM name or ID
        #[arg(long)]
        vm: String,
        /// Optional backup name
        #[arg(long)]
        name: Option<String>,
        /// Optional storage pool name or ID
        #[arg(long)]
        pool: Option<String>,
    },
    /// Create a control-plane database backup
    Database {
        /// Optional backup name
        #[arg(long)]
        name: Option<String>,
        /// Optional storage pool name or ID
        #[arg(long)]
        pool: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BackupKind {
    Vm,
    Database,
}

impl BackupKind {
    fn as_api_str(self) -> &'static str {
        match self {
            Self::Vm => "vm",
            Self::Database => "database",
        }
    }
}

#[derive(Tabled)]
struct BackupRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    backup_type: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Target")]
    target: String,
    #[tabled(rename = "Created")]
    created_at: String,
}

fn backup_target(backup: &api::models::Backup) -> String {
    match backup.backup_type.as_str() {
        "vm" => backup
            .vm_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".to_string()),
        _ => backup
            .database_name
            .clone()
            .unwrap_or_else(|| "control-plane-db".to_string()),
    }
}

pub async fn run(args: BackupArgs, client: &Client, output: OutputFormat) -> anyhow::Result<()> {
    match args.command {
        BackupCommand::List { name, backup_type } => {
            let backups = api::backups::list(
                client,
                name.as_deref(),
                backup_type.map(BackupKind::as_api_str),
            )
            .await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&backups, output)?;
            } else {
                let rows: Vec<BackupRow> = backups
                    .iter()
                    .map(|backup| BackupRow {
                        id: backup.id.to_string(),
                        name: backup.name.clone(),
                        backup_type: backup.backup_type.clone(),
                        status: backup.status.clone(),
                        target: backup_target(backup),
                        created_at: backup.created_at.clone(),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }
        BackupCommand::Get { backup } => {
            let backup_id = resolve_backup_id(client, &backup).await?;
            let backup = api::backups::get(client, backup_id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&backup, output)?;
            } else {
                println!("ID:          {}", backup.id);
                println!("Name:        {}", backup.name);
                println!("Type:        {}", backup.backup_type);
                println!("Status:      {}", backup.status);
                println!("Target:      {}", backup_target(&backup));
                println!("Object:      {}", backup.storage_object_id);
                if let Some(snapshot_id) = backup.snapshot_id {
                    println!("Snapshot:    {snapshot_id}");
                }
                if let Some(error) = &backup.error_message {
                    println!("Error:       {error}");
                }
                println!("Created:     {}", backup.created_at);
                println!("Updated:     {}", backup.updated_at);
            }
        }
        BackupCommand::Create { command } => {
            let request = match command {
                CreateBackupCommand::Vm { vm, name, pool } => {
                    let vm_id = resolve_vm_id(client, &vm).await?;
                    let storage_pool_id = match pool {
                        Some(pool) => Some(resolve_pool_id(client, &pool).await?),
                        None => None,
                    };
                    CreateBackupRequest {
                        name,
                        storage_pool_id,
                        backup_type: "vm".to_string(),
                        vm_id: Some(vm_id),
                    }
                }
                CreateBackupCommand::Database { name, pool } => {
                    let storage_pool_id = match pool {
                        Some(pool) => Some(resolve_pool_id(client, &pool).await?),
                        None => None,
                    };
                    CreateBackupRequest {
                        name,
                        storage_pool_id,
                        backup_type: "database".to_string(),
                        vm_id: None,
                    }
                }
            };

            let backup = api::backups::create(client, &request).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&backup, output)?;
            } else {
                println!("Backup:      {}", backup.id);
                println!("Name:        {}", backup.name);
                println!("Type:        {}", backup.backup_type);
                println!("Status:      {}", backup.status);
                println!("Target:      {}", backup_target(&backup));
            }
        }
        BackupCommand::Restore { backup } => {
            let backup_id = resolve_backup_id(client, &backup).await?;
            let restored = api::backups::restore(client, backup_id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&restored, output)?;
            } else {
                println!("Backup:      {}", restored.backup_id);
                println!("Type:        {}", restored.backup_type);
                if let Some(vm_id) = restored.vm_id {
                    println!("VM:          {vm_id}");
                }
                if let Some(database_name) = restored.database_name {
                    println!("Database:    {database_name}");
                }
            }
        }
    }

    Ok(())
}
