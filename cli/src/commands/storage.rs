use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};
use uuid::Uuid;

use crate::api::jobs;

use crate::{
    api::{
        self,
        models::{CreateDiskRequest, ImportToPoolRequest, NewStorageObject, NewStoragePool},
    },
    client::Client,
};

use super::{
    OutputFormat, format_bytes, parse_size, print_output, resolve_host_id, resolve_object_id,
    resolve_pool_id,
};

/// Poll a job to completion, printing progress to stderr. Returns an error if the job fails.
async fn poll_job_to_completion(client: &Client, job_id: Uuid, label: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    loop {
        let job = jobs::get(client, job_id).await?;
        match job.status.as_str() {
            "completed" => {
                eprintln!("\r[completed]                    ");
                return Ok(());
            }
            "failed" => {
                return Err(anyhow::anyhow!(
                    "{label} job {job_id} failed: {}",
                    job.error.unwrap_or_else(|| "unknown error".to_string())
                ));
            }
            status => {
                eprint!("\r[{status}] {}%   ", job.progress.unwrap_or(0));
                let _ = std::io::stderr().flush();
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
}

fn active_hosts(hosts: &[api::models::Host]) -> impl Iterator<Item = &api::models::Host> {
    hosts
        .iter()
        .filter(|host| host.status.eq_ignore_ascii_case("up"))
}

// Storage pools

#[derive(Args)]
pub struct StoragePoolArgs {
    #[command(subcommand)]
    command: StoragePoolCommand,
}

#[derive(Subcommand)]
enum StoragePoolCommand {
    /// List all storage pools
    List,
    /// Get details of a storage pool
    Get {
        /// Pool name or ID
        pool: String,
    },
    /// Create a storage pool
    Create {
        /// Pool name
        #[arg(long)]
        name: String,
        /// Pool type (local, nfs, or overlaybd)
        #[arg(long, value_name = "TYPE")]
        pool_type: String,
        /// Capacity in bytes
        #[arg(long)]
        capacity: Option<i64>,
        /// Pool config as JSON (e.g. '{"url":"http://registry:5000"}' for overlaybd)
        #[arg(long, value_name = "JSON")]
        config: Option<String>,
        /// Host to attach this pool to (name or ID, for local pools)
        #[arg(long)]
        host: Option<String>,
    },
    /// Delete a storage pool
    Delete {
        /// Pool name or ID
        pool: String,
    },
    /// Attach a host to a storage pool
    AttachHost {
        /// Pool name or ID
        pool: String,
        /// Host name or ID (optional if --all is used)
        host: Option<String>,
        /// Attach all active hosts
        #[arg(long)]
        all: bool,
    },
    /// Detach a host from a storage pool
    DetachHost {
        /// Pool name or ID
        pool: String,
        /// Host name or ID
        host: String,
    },
    /// Create a disk in the pool (blank, or populated from a source URL)
    CreateDisk {
        /// Pool name or ID
        #[arg(long)]
        pool: String,
        /// Name for the resulting disk storage object
        #[arg(long)]
        name: String,
        /// Disk size (e.g. 10GiB, 20GB, 53687091200). Required for blank disks.
        #[arg(long)]
        size: Option<String>,
        /// URL to populate the disk from (e.g. a cloud image). Makes the operation async.
        #[arg(long)]
        source: Option<String>,
        /// Reserve blocks upfront with fallocate (default: sparse)
        #[arg(long)]
        preallocate: bool,
    },
    /// Import an OCI image into the pool (convert to OverlayBD)
    Import {
        /// Pool name or ID
        #[arg(long)]
        pool: String,
        /// OCI image reference (e.g. public.ecr.aws/docker/library/alpine:latest)
        #[arg(long)]
        image_ref: String,
        /// Name for the resulting storage object
        #[arg(long)]
        name: String,
    },
}

#[derive(Tabled)]
struct PoolRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    pool_type: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Capacity")]
    capacity: String,
    #[tabled(rename = "Allocated")]
    allocated: String,
}

pub async fn run_pool(
    args: StoragePoolArgs,
    client: &Client,
    output: OutputFormat,
) -> anyhow::Result<()> {
    match args.command {
        StoragePoolCommand::List => {
            let pools = api::storage::list_pools(client, None).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&pools, output)?;
            } else {
                let rows: Vec<PoolRow> = pools
                    .iter()
                    .map(|p| PoolRow {
                        id: p.id.to_string(),
                        name: p.name.clone(),
                        pool_type: p.pool_type.clone(),
                        status: p.status.clone(),
                        capacity: p
                            .capacity_bytes
                            .map(format_bytes)
                            .unwrap_or_else(|| "-".to_string()),
                        allocated: p
                            .allocated_bytes
                            .map(format_bytes)
                            .unwrap_or_else(|| "-".to_string()),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }

        StoragePoolCommand::Get { pool } => {
            let id = resolve_pool_id(client, &pool).await?;
            let pool = api::storage::get_pool(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&pool, output)?;
            } else {
                println!("ID:       {}", pool.id);
                println!("Name:     {}", pool.name);
                println!("Type:     {}", pool.pool_type);
                println!("Status:   {}", pool.status);
                println!(
                    "Capacity: {}",
                    pool.capacity_bytes
                        .map(format_bytes)
                        .unwrap_or_else(|| "-".to_string())
                );
                println!(
                    "Used:     {}",
                    pool.allocated_bytes
                        .map(format_bytes)
                        .unwrap_or_else(|| "-".to_string())
                );
            }
        }

        StoragePoolCommand::Create {
            name,
            pool_type,
            capacity,
            config,
            host,
        } => {
            let config = match config {
                Some(s) => serde_json::from_str(&s)
                    .map_err(|e| anyhow::anyhow!("Invalid JSON for --config: {e}"))?,
                None => serde_json::json!({}),
            };
            let new_pool = NewStoragePool {
                name,
                pool_type,
                config,
                capacity_bytes: capacity,
            };
            let id = api::storage::create_pool(client, &new_pool).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "pool_id": id }), output)?;
            } else {
                println!("Created storage pool: {id}");
            }

            if let Some(host_name) = host {
                let pool_uuid = uuid::Uuid::parse_str(&id)
                    .map_err(|e| anyhow::anyhow!("Invalid pool UUID: {}", e))?;
                let host_id = resolve_host_id(client, &host_name).await?;
                api::storage::attach_host_to_pool(client, pool_uuid, host_id).await?;
                if matches!(output, OutputFormat::Table) {
                    println!("Attached host: {host_name}");
                }
            }
        }

        StoragePoolCommand::Delete { pool } => {
            let id = resolve_pool_id(client, &pool).await?;
            api::storage::delete_pool(client, id).await?;
            println!("Deleted storage pool: {id}");
        }

        StoragePoolCommand::AttachHost { pool, host, all } => {
            let pool_id = resolve_pool_id(client, &pool).await?;
            if all {
                let hosts = api::hosts::list(client, None, None).await?;
                for h in active_hosts(&hosts) {
                    if let Err(e) = api::storage::attach_host_to_pool(client, pool_id, h.id).await {
                        eprintln!(
                            "Warning: Failed to attach host {} ({}): {}",
                            h.name, h.id, e
                        );
                    } else if matches!(output, OutputFormat::Table) {
                        println!("Attached host: {}", h.name);
                    }
                }
            } else if let Some(host_name) = host {
                let host_id = resolve_host_id(client, &host_name).await?;
                api::storage::attach_host_to_pool(client, pool_id, host_id).await?;
                if !matches!(output, OutputFormat::Table) {
                    print_output(
                        &serde_json::json!({ "pool_id": pool_id, "host_id": host_id }),
                        output,
                    )?;
                } else {
                    println!("Attached host {host_name} to pool {pool}");
                }
            } else {
                return Err(anyhow::anyhow!("Must provide either [HOST] or --all"));
            }
        }

        StoragePoolCommand::DetachHost { pool, host } => {
            let pool_id = resolve_pool_id(client, &pool).await?;
            let host_id = resolve_host_id(client, &host).await?;
            api::storage::detach_host_from_pool(client, pool_id, host_id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(
                    &serde_json::json!({ "pool_id": pool_id, "host_id": host_id }),
                    output,
                )?;
            } else {
                println!("Detached host {host} from pool {pool}");
            }
        }

        StoragePoolCommand::CreateDisk {
            pool,
            name,
            size,
            source,
            preallocate,
        } => {
            let pool_id = resolve_pool_id(client, &pool).await?;

            let size_bytes = match (&size, &source) {
                (None, None) => {
                    return Err(anyhow::anyhow!(
                        "--size is required when --source is not provided"
                    ));
                }
                (None, Some(_)) => None,
                (Some(s), _) => Some(parse_size(s)?),
            };

            let req = CreateDiskRequest {
                name,
                size_bytes,
                source_url: source.clone(),
                preallocate,
            };
            let resp = api::storage::create_disk(client, pool_id, &req).await?;

            if let Some(job_id) = resp.job_id {
                if !matches!(output, OutputFormat::Table) {
                    print_output(&resp, output)?;
                } else {
                    println!("Disk object: {}", resp.storage_object_id);
                    println!("Download job: {job_id}");
                    poll_job_to_completion(client, job_id, "Disk creation").await?;
                }
            } else if !matches!(output, OutputFormat::Table) {
                print_output(&resp, output)?;
            } else {
                println!("Created disk: {}", resp.storage_object_id);
            }
        }

        StoragePoolCommand::Import {
            pool,
            image_ref,
            name,
        } => {
            let pool_id = resolve_pool_id(client, &pool).await?;
            let req = ImportToPoolRequest { name, image_ref };
            let resp = api::storage::import_to_pool(client, pool_id, &req).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&resp, output)?;
            } else {
                println!("Import job: {}", resp.job_id);
                println!("Storage object: {}", resp.storage_object_id);
                poll_job_to_completion(client, resp.job_id, "Import").await?;
            }
        }
    }

    Ok(())
}

// Storage objects

#[derive(Args)]
pub struct StorageObjectArgs {
    #[command(subcommand)]
    command: StorageObjectCommand,
}

#[derive(Subcommand)]
enum StorageObjectCommand {
    /// List all storage objects
    List,
    /// Get details of a storage object
    Get {
        /// Object name or ID
        object: String,
    },
    /// Create a storage object
    Create {
        /// Object name
        #[arg(long)]
        name: String,
        /// Storage pool name or ID (optional; a random active pool is chosen if omitted)
        #[arg(long)]
        pool: Option<String>,
        /// Object type (disk, kernel, initrd, iso, snapshot, oci_image)
        #[arg(long, value_name = "TYPE")]
        object_type: String,
        /// Size in bytes
        #[arg(long)]
        size: i64,
        /// Optional parent object ID
        #[arg(long)]
        parent: Option<Uuid>,
    },
    /// Delete a storage object
    Delete {
        /// Object name or ID
        object: String,
    },
}

#[derive(Tabled)]
struct ObjectRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Pool")]
    pool_id: String,
    #[tabled(rename = "Type")]
    object_type: String,
    #[tabled(rename = "Size")]
    size: String,
}

pub async fn run_object(
    args: StorageObjectArgs,
    client: &Client,
    output: OutputFormat,
) -> anyhow::Result<()> {
    match args.command {
        StorageObjectCommand::List => {
            let objects = api::storage::list_objects(client, None).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&objects, output)?;
            } else {
                let rows: Vec<ObjectRow> = objects
                    .iter()
                    .map(|o| ObjectRow {
                        id: o.id.to_string(),
                        name: o.name.clone(),
                        pool_id: o.storage_pool_id.to_string(),
                        object_type: o.object_type.clone(),
                        size: format_bytes(o.size_bytes),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }

        StorageObjectCommand::Get { object } => {
            let id = resolve_object_id(client, &object).await?;
            let obj = api::storage::get_object(client, id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&obj, output)?;
            } else {
                println!("ID:   {}", obj.id);
                println!("Name: {}", obj.name);
                println!("Pool: {}", obj.storage_pool_id);
                println!("Type: {}", obj.object_type);
                println!("Size: {}", format_bytes(obj.size_bytes));
                if let Some(p) = obj.parent_id {
                    println!("Parent: {p}");
                }
            }
        }

        StorageObjectCommand::Create {
            name,
            pool,
            object_type,
            size,
            parent,
        } => {
            let pool_id = match pool {
                Some(ref s) => Some(resolve_pool_id(client, s).await?),
                None => None,
            };
            let new_obj = NewStorageObject {
                name,
                storage_pool_id: pool_id,
                object_type,
                size_bytes: size,
                config: serde_json::json!({}),
                parent_id: parent,
            };
            let id = api::storage::create_object(client, &new_obj).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&serde_json::json!({ "object_id": id }), output)?;
            } else {
                println!("Created storage object: {id}");
            }
        }

        StorageObjectCommand::Delete { object } => {
            let id = resolve_object_id(client, &object).await?;
            api::storage::delete_object(client, id).await?;
            println!("Deleted storage object: {id}");
        }
    }

    Ok(())
}
