use anyhow::anyhow;
use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};
use uuid::Uuid;

use crate::{
    api::{
        self,
        models::{
            AttachDiskRequest, CreateSnapshotRequest, CreateVmResult, HotplugNicRequest, NewVm,
            NewVmNetwork, RestoreRequest, VmMigrateRequest,
        },
    },
    client::Client,
    console,
};

use super::{
    OutputFormat, format_bytes, print_output, resolve_boot_source_id, resolve_host_id,
    resolve_network_id, resolve_object_id, resolve_vm_id,
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
        /// Path to cloud-init user-data file (triggers NoCloud seed disk attachment)
        #[arg(long, value_name = "FILE")]
        cloud_init_user_data: Option<std::path::PathBuf>,
        /// Path to cloud-init meta-data file (auto-generated from VM name/id if omitted)
        #[arg(long, value_name = "FILE", requires = "cloud_init_user_data")]
        cloud_init_meta_data: Option<std::path::PathBuf>,
        /// Path to cloud-init network-config file (suppresses kernel ip= params when set)
        #[arg(long, value_name = "FILE", requires = "cloud_init_user_data")]
        cloud_init_network_config: Option<std::path::PathBuf>,
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
    /// Remove a disk from a VM (hotunplugs if VM is running)
    RemoveDisk {
        /// VM name or ID
        vm: String,
        /// Disk logical name to remove (e.g. "disk0")
        #[arg(long)]
        device_id: String,
    },
    /// Add a NIC to a VM (hotplugs if VM is running)
    AddNic {
        /// VM name or ID
        vm: String,
        /// Device ID for the new NIC (e.g. "net1"); auto-generated if omitted
        #[arg(long, default_value = "")]
        device_id: String,
        /// Network name or ID for managed IP allocation
        #[arg(long)]
        network: Option<String>,
        /// Static IP address (requires --network)
        #[arg(long, requires = "network")]
        ip: Option<String>,
        /// Guest MAC address
        #[arg(long)]
        mac: Option<String>,
        /// Pre-created TAP device name
        #[arg(long)]
        tap: Option<String>,
        /// MTU override
        #[arg(long)]
        mtu: Option<i32>,
    },
    /// Remove a NIC from a VM (hotunplugs if VM is running)
    RemoveNic {
        /// VM name or ID
        vm: String,
        /// NIC device ID to remove (e.g. "net0")
        #[arg(long)]
        device_id: String,
    },
    /// Live-migrate a running VM to another host (NFS-backed storage only)
    Migrate {
        /// VM name or ID
        vm: String,
        /// Destination host name or ID
        #[arg(long)]
        host: String,
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
        /// Storage pool ID to store the snapshot in (auto-selected if omitted)
        #[arg(long)]
        pool: Option<Uuid>,
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

pub async fn run(args: VmArgs, client: &Client, output: OutputFormat) -> anyhow::Result<()> {
    match args.command {
        VmCommand::List => {
            let vms = api::vms::list(client).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&vms, output)?;
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
            if !matches!(output, OutputFormat::Table) {
                print_output(&vm, output)?;
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
            cloud_init_user_data,
            cloud_init_meta_data,
            cloud_init_network_config,
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
            let read_file = |path: Option<std::path::PathBuf>| -> anyhow::Result<Option<String>> {
                path.map(|p| {
                    std::fs::read_to_string(&p)
                        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", p.display(), e))
                })
                .transpose()
            };
            let ci_user_data = read_file(cloud_init_user_data)?;
            let ci_meta_data = read_file(cloud_init_meta_data)?;
            let ci_network_config = read_file(cloud_init_network_config)?;

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
                cloud_init_user_data: ci_user_data,
                cloud_init_meta_data: ci_meta_data,
                cloud_init_network_config: ci_network_config,
                config: serde_json::json!({}),
            };

            let result = api::vms::create(client, &new_vm).await?;
            match result {
                CreateVmResult::Created(vm_id) => {
                    if !matches!(output, OutputFormat::Table) {
                        print_output(&serde_json::json!({ "vm_id": vm_id }), output)?;
                    } else {
                        println!("Created VM: {}", new_vm.name);
                    }
                }
                CreateVmResult::Accepted { vm_id, job_id } => {
                    if !matches!(output, OutputFormat::Table) {
                        print_output(
                            &serde_json::json!({ "vm_id": vm_id, "job_id": job_id }),
                            output,
                        )?;
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
            if !matches!(output, OutputFormat::Table) {
                print_output(&resp, output)?;
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
            if !matches!(output, OutputFormat::Table) {
                print_output(&disk, output)?;
            } else {
                println!(
                    "Attached disk {} (object={}, name={}) to VM {}",
                    disk.id, object_id, disk.logical_name, vm_id
                );
            }
        }

        VmCommand::RemoveDisk { vm, device_id } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            api::vms::remove_disk(client, vm_id, &device_id).await?;
            println!("Removed disk {device_id} from VM {vm}");
        }

        VmCommand::AddNic {
            vm,
            device_id,
            network,
            ip,
            mac,
            tap,
            mtu,
        } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            let network_id = match network {
                Some(ref s) => Some(resolve_network_id(client, s).await?),
                None => None,
            };
            let req = HotplugNicRequest {
                id: device_id,
                network_id,
                ip,
                mac,
                tap,
                mtu,
            };
            let nic = api::vms::add_nic(client, vm_id, &req).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&nic, output)?;
            } else {
                println!("Added NIC {} to VM {vm}", nic.device_id);
                if let Some(ip) = &nic.ip_address {
                    println!("IP: {ip}");
                }
            }
        }

        VmCommand::RemoveNic { vm, device_id } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            api::vms::remove_nic(client, vm_id, &device_id).await?;
            println!("Removed NIC {device_id} from VM {vm}");
        }

        VmCommand::Migrate { vm, host } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            let host_id = resolve_host_id(client, &host).await?;
            let req = VmMigrateRequest {
                target_host_id: host_id,
            };
            let resp = api::vms::migrate(client, vm_id, &req).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&resp, output)?;
            } else {
                println!("Migrating VM: {vm}");
                println!("Job:          {}", resp.job_id);
                poll_job(client, resp.job_id).await?;
            }
        }

        VmCommand::Snapshot { command } => match command {
            SnapshotCommand::Create { vm, name, pool } => {
                let id = resolve_vm_id(client, &vm).await?;
                let req = CreateSnapshotRequest {
                    name,
                    storage_pool_id: pool,
                };
                let snapshot = api::vms::create_snapshot(client, id, &req).await?;
                if !matches!(output, OutputFormat::Table) {
                    print_output(&snapshot, output)?;
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
                if !matches!(output, OutputFormat::Table) {
                    print_output(&snapshots, output)?;
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
                if !matches!(output, OutputFormat::Table) {
                    print_output(&restored, output)?;
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
