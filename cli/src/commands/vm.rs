use anyhow::anyhow;
use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};
use uuid::Uuid;

use crate::{
    api::{
        self,
        models::{
            AttachDiskRequest, CreateSnapshotRequest, CreateVmResult, NewVm, NewVmNetwork,
            RestoreRequest,
        },
    },
    client::Client,
    console,
};

use super::{
    format_bytes, print_json, resolve_boot_source_id, resolve_network_id, resolve_object_id,
    resolve_vm_id,
};

#[derive(Args)]
pub struct VmArgs {
    #[command(subcommand)]
    command: VmCommand,
}

#[derive(Subcommand)]
enum VmCommand {
    /// List all VMs
    List,
    /// Get details of a specific VM
    Get {
        /// VM name or ID
        vm: String,
    },
    /// Create a new VM
    Create {
        /// VM name
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
        /// Boot source name or ID
        #[arg(long)]
        boot_source: Option<String>,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// OCI image reference (triggers async creation)
        #[arg(long)]
        image_ref: Option<String>,
        /// Boot mode: kernel (default) or firmware
        #[arg(long, default_value = "kernel")]
        boot_mode: String,
        /// Network name or ID to attach the VM to (allocates an IP automatically)
        #[arg(long)]
        network: Option<String>,
        /// Static IP address to assign to the VM (requires --network)
        #[arg(long, requires = "network")]
        ip: Option<String>,
    },
    /// Delete a VM
    Delete {
        /// VM name or ID
        vm: String,
    },
    /// Start a VM
    Start {
        /// VM name or ID
        vm: String,
    },
    /// Stop a VM
    Stop {
        /// VM name or ID
        vm: String,
    },
    /// Pause a VM
    Pause {
        /// VM name or ID
        vm: String,
    },
    /// Resume a paused VM
    Resume {
        /// VM name or ID
        vm: String,
    },
    /// Print the VM's console log
    Console {
        /// VM name or ID
        vm: String,
    },
    /// Attach an interactive WebSocket console to a running VM
    Attach {
        /// VM name or ID
        vm: String,
    },
    /// Attach an OverlayBD storage object as a disk on a VM
    AttachDisk {
        /// VM name or ID
        vm: String,
        /// Storage object name or ID to attach as a disk
        #[arg(long)]
        object: String,
        /// Logical device name inside the VM (e.g. "vda"); auto-generated if omitted
        #[arg(long)]
        logical_name: Option<String>,
        /// Boot order priority (lower = higher priority; default: 0)
        #[arg(long)]
        boot_order: Option<i32>,
    },
    /// Manage VM snapshots
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
}

#[derive(Subcommand)]
enum SnapshotCommand {
    /// Create a snapshot of a running VM
    Create {
        /// VM name or ID
        vm: String,
        /// Optional name for the snapshot (auto-generated if omitted)
        #[arg(long)]
        name: Option<String>,
    },
    /// List snapshots for a VM
    List {
        /// VM name or ID
        vm: String,
    },
    /// Restore a VM from a snapshot
    Restore {
        /// VM name or ID
        vm: String,
        /// Snapshot ID to restore from
        #[arg(long)]
        snapshot: Uuid,
    },
}

#[derive(Tabled)]
struct SnapshotRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Created")]
    created_at: String,
}

#[derive(Tabled)]
struct VmRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Host")]
    host_id: String,
    #[tabled(rename = "vCPUs")]
    vcpus: String,
    #[tabled(rename = "Memory")]
    memory: String,
    #[tabled(rename = "Image")]
    image_ref: String,
}

pub async fn run(args: VmArgs, client: &Client, json: bool) -> anyhow::Result<()> {
    match args.command {
        VmCommand::List => {
            let vms = api::vms::list(client).await?;
            if json {
                print_json(&vms)?;
            } else {
                let rows: Vec<VmRow> = vms
                    .iter()
                    .map(|vm| VmRow {
                        id: vm.id.to_string(),
                        name: vm.name.clone(),
                        status: vm.status.clone(),
                        host_id: vm
                            .host_id
                            .map(|h| h.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        vcpus: format!("{}/{}", vm.boot_vcpus, vm.max_vcpus),
                        memory: format_bytes(vm.memory_size),
                        image_ref: vm.image_ref.clone().unwrap_or_else(|| "-".to_string()),
                    })
                    .collect();
                println!("{}", Table::new(rows).with(Style::psql()));
            }
        }

        VmCommand::Get { vm } => {
            let id = resolve_vm_id(client, &vm).await?;
            let vm = api::vms::get(client, id).await?;
            if json {
                print_json(&vm)?;
            } else {
                println!("ID:          {}", vm.id);
                println!("Name:        {}", vm.name);
                println!("Status:      {}", vm.status);
                println!("Hypervisor:  {}", vm.hypervisor);
                println!(
                    "Host:        {}",
                    vm.host_id
                        .map(|h| h.to_string())
                        .unwrap_or_else(|| "-".to_string())
                );
                println!("Boot mode:   {}", vm.boot_mode);
                println!("vCPUs:       {}/{}", vm.boot_vcpus, vm.max_vcpus);
                println!("Memory:      {}", format_bytes(vm.memory_size));
                if let Some(bs) = vm.boot_source_id {
                    println!("Boot source: {bs}");
                }
                if let Some(desc) = &vm.description {
                    println!("Description: {desc}");
                }
                if let Some(img) = &vm.image_ref {
                    println!("Image:       {img}");
                }
            }
        }

        VmCommand::Create {
            name,
            vcpus,
            max_vcpus,
            memory,
            boot_source,
            description,
            image_ref,
            boot_mode,
            network,
            ip,
        } => {
            let boot_source_id = match boot_source {
                Some(ref s) => Some(resolve_boot_source_id(client, s).await?),
                None => None,
            };
            let boot_mode_opt = if boot_mode == "kernel" {
                None
            } else {
                Some(boot_mode)
            };
            // When --ip is given we pass an explicit networks entry so the server
            // uses that IP instead of auto-allocating one.
            let (network_id, networks) = match network {
                None => (None, None),
                Some(ref s) => {
                    let nid = resolve_network_id(client, s).await?;
                    if let Some(addr) = ip {
                        let iface = NewVmNetwork {
                            id: "net0".to_string(),
                            network_id: Some(nid),
                            ip: Some(addr),
                        };
                        (None, Some(vec![iface]))
                    } else {
                        (Some(nid), None)
                    }
                }
            };
            let new_vm = NewVm {
                name,
                hypervisor: "cloud_hv".to_string(),
                boot_vcpus: vcpus,
                max_vcpus: max_vcpus.unwrap_or(vcpus),
                memory_size: memory,
                boot_source_id,
                boot_mode: boot_mode_opt,
                description,
                image_ref: image_ref.clone(),
                network_id,
                networks,
                config: serde_json::json!({}),
            };

            let result = api::vms::create(client, &new_vm).await?;
            match result {
                CreateVmResult::Created(vm_id) => {
                    if json {
                        print_json(&serde_json::json!({ "vm_id": vm_id }))?;
                    } else {
                        println!("Created VM: {}", new_vm.name);
                    }
                }
                CreateVmResult::Accepted { vm_id, job_id } => {
                    if json {
                        print_json(&serde_json::json!({ "vm_id": vm_id, "job_id": job_id }))?;
                    } else {
                        println!("Creating VM: {}", new_vm.name);
                        println!("Job:         {job_id}");
                        poll_job(client, job_id).await?;
                    }
                }
            }
        }

        VmCommand::Delete { vm } => {
            let id = resolve_vm_id(client, &vm).await?;
            api::vms::delete(client, id).await?;
            println!("Deleted VM: {vm}");
        }

        VmCommand::Start { vm } => {
            let id = resolve_vm_id(client, &vm).await?;
            let resp = api::vms::start(client, id).await?;
            if json {
                print_json(&resp)?;
            } else {
                println!("Starting VM: {vm}");
                println!("Job:         {}", resp.job_id);
                poll_job(client, resp.job_id).await?;
            }
        }

        VmCommand::Stop { vm } => {
            let id = resolve_vm_id(client, &vm).await?;
            api::vms::stop(client, id).await?;
            println!("Stopped VM: {vm}");
        }

        VmCommand::Pause { vm } => {
            let id = resolve_vm_id(client, &vm).await?;
            api::vms::pause(client, id).await?;
            println!("Paused VM: {vm}");
        }

        VmCommand::Resume { vm } => {
            let id = resolve_vm_id(client, &vm).await?;
            api::vms::resume(client, id).await?;
            println!("Resumed VM: {vm}");
        }

        VmCommand::Console { vm } => {
            let id = resolve_vm_id(client, &vm).await?;
            let log = api::vms::console_log(client, id).await?;
            print!("{log}");
        }

        VmCommand::Attach { vm } => {
            let id = resolve_vm_id(client, &vm).await?;
            console::attach(client.base_url(), id).await?;
        }

        VmCommand::AttachDisk {
            vm,
            object,
            logical_name,
            boot_order,
        } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            let object_id = resolve_object_id(client, &object).await?;
            let req = AttachDiskRequest {
                storage_object_id: object_id,
                logical_name,
                boot_order,
            };
            let disk = api::vms::attach_disk(client, vm_id, &req).await?;
            if json {
                print_json(&disk)?;
            } else {
                println!(
                    "Attached disk {} (object={}, name={}) to VM {}",
                    disk.id, object_id, disk.logical_name, vm_id
                );
            }
        }

        VmCommand::Snapshot { command } => match command {
            SnapshotCommand::Create { vm, name } => {
                let id = resolve_vm_id(client, &vm).await?;
                let req = CreateSnapshotRequest { name };
                let snapshot = api::vms::create_snapshot(client, id, &req).await?;
                if json {
                    print_json(&snapshot)?;
                } else {
                    println!("Snapshot: {}", snapshot.id);
                    println!("Name:     {}", snapshot.name);
                    println!("Status:   {}", snapshot.status);
                    println!("Created:  {}", snapshot.created_at);
                }
            }

            SnapshotCommand::List { vm } => {
                let id = resolve_vm_id(client, &vm).await?;
                let snapshots = api::vms::list_snapshots(client, id).await?;
                if json {
                    print_json(&snapshots)?;
                } else {
                    let rows: Vec<SnapshotRow> = snapshots
                        .iter()
                        .map(|s| SnapshotRow {
                            id: s.id.to_string(),
                            name: s.name.clone(),
                            status: s.status.clone(),
                            created_at: s.created_at.clone(),
                        })
                        .collect();
                    println!("{}", Table::new(rows).with(Style::psql()));
                }
            }

            SnapshotCommand::Restore { vm, snapshot } => {
                let id = resolve_vm_id(client, &vm).await?;
                let req = RestoreRequest {
                    snapshot_id: snapshot,
                };
                let restored = api::vms::restore(client, id, &req).await?;
                if json {
                    print_json(&restored)?;
                } else {
                    println!("Restored VM: {}", restored.name);
                    println!("Status:      {}", restored.status);
                }
            }
        },
    }

    Ok(())
}

/// Poll a job until it completes or fails, printing progress to stderr.
async fn poll_job(client: &Client, job_id: Uuid) -> anyhow::Result<()> {
    use std::io::Write as _;
    loop {
        let job = api::jobs::get(client, job_id).await?;
        match job.status.as_str() {
            "completed" => {
                eprintln!("\r[completed]                    ");
                return Ok(());
            }
            "failed" => {
                return Err(anyhow!(
                    "Job {job_id} failed: {}",
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
