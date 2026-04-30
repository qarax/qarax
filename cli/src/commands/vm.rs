use anyhow::anyhow;
use clap::{Args, Subcommand};
use tabled::{Table, Tabled, settings::Style};
use uuid::Uuid;

use crate::{
    api::{
        self,
        models::{
            AttachDiskRequest, CommitVmRequest, CreateSnapshotRequest, CreateVmResult,
            DiskResizeRequest, HotplugNicRequest, NewVm, NewVmNetwork, RestoreRequest,
            VmImagePreflightRequest, VmMigrateRequest, VmResizeRequest,
        },
    },
    client::Client,
    console,
};

use super::{
    OutputFormat, build_accelerator_config, format_bytes, parse_key_value_pairs, print_output,
    resolve_boot_source_id, resolve_host_id, resolve_instance_type_id, resolve_network_id,
    resolve_object_id, resolve_pool_id, resolve_security_group_id, resolve_vm_id,
    resolve_vm_template_id,
};

#[derive(Args)]
pub struct VmArgs {
    #[command(subcommand)]
    command: VmCommand,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum VmCommand {
    /// List all VMs
    List {
        /// Filter by tag (can be repeated; VMs must have all specified tags)
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
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
        /// VM tag to attach. Repeat to set multiple tags.
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Number of vCPUs at boot
        #[arg(long)]
        vcpus: Option<i32>,
        /// Maximum vCPUs (defaults to --vcpus)
        #[arg(long)]
        max_vcpus: Option<i32>,
        /// Memory size (e.g. 2GiB, 512MiB, 1073741824)
        #[arg(long, value_parser = super::parse_size)]
        memory: Option<i64>,
        /// VM template name or ID
        #[arg(long)]
        template: Option<String>,
        /// Instance type name or ID
        #[arg(long)]
        instance_type: Option<String>,
        /// Hypervisor type
        #[arg(long)]
        hypervisor: Option<String>,
        /// Target architecture (e.g. x86_64, aarch64, riscv64)
        #[arg(long)]
        architecture: Option<String>,
        /// Boot source name or ID
        #[arg(long)]
        boot_source: Option<String>,
        /// Root disk storage object name or ID
        #[arg(long)]
        root_disk: Option<String>,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// OCI image reference (triggers async creation)
        #[arg(long)]
        image_ref: Option<String>,
        /// Boot mode: 'kernel' (direct Linux boot via kernel+initramfs, default) or
        /// 'firmware' (UEFI/EDK2 — required for cloud images, Windows, or any disk
        /// with its own EFI bootloader; use with --root-disk pointing to a UEFI image).
        #[arg(long, default_value = "kernel")]
        boot_mode: String,
        /// Network name or ID to attach the VM to (allocates an IP automatically)
        #[arg(long)]
        network: Option<String>,
        /// Static IP address to assign to the VM (requires --network)
        #[arg(long, requires = "network")]
        ip: Option<String>,
        /// Security group name or ID to bind to the VM. Repeat to attach multiple groups.
        #[arg(long = "security-group")]
        security_groups: Vec<String>,
        /// Enable TCP Segmentation Offload for the primary NIC
        #[arg(long, requires = "network")]
        offload_tso: Option<bool>,
        /// Enable UDP Fragmentation Offload for the primary NIC
        #[arg(long, requires = "network")]
        offload_ufo: Option<bool>,
        /// Enable checksum offload for the primary NIC
        #[arg(long, requires = "network")]
        offload_csum: Option<bool>,
        /// Path to cloud-init user-data file (triggers NoCloud seed disk attachment)
        #[arg(long, value_name = "FILE")]
        cloud_init_user_data: Option<std::path::PathBuf>,
        /// Path to cloud-init meta-data file (auto-generated from VM name/id if omitted)
        #[arg(long, value_name = "FILE", requires = "cloud_init_user_data")]
        cloud_init_meta_data: Option<std::path::PathBuf>,
        /// Path to cloud-init network-config file (suppresses kernel ip= params when set)
        #[arg(long, value_name = "FILE", requires = "cloud_init_user_data")]
        cloud_init_network_config: Option<std::path::PathBuf>,
        /// Number of GPUs to request (enables GPU-aware scheduling)
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
        /// Pin VM to a specific host NUMA node (0-indexed). Ignored when --gpu-count is set
        /// (GPU-local NUMA is used automatically in that case).
        #[arg(long)]
        numa_node: Option<i32>,
        /// Storage pool name or ID for persistent OverlayBD upper layer (requires --image-ref).
        /// When set, writes to the OCI-booted root disk survive VM deletion.
        /// Pool must be Local or NFS and attached to the host running the VM.
        #[arg(long, requires = "image_ref")]
        persistent_upper_pool: Option<String>,
        /// Require a host from this reservation class
        #[arg(long)]
        reservation_class: Option<String>,
        /// Require a host label in key=value form. Repeat to require multiple labels.
        #[arg(long = "require-host-label")]
        required_host_labels: Vec<String>,
        /// Prefer hosts with this label set in key=value form. Repeat for multiple labels.
        #[arg(long = "prefer-host-label")]
        preferred_host_labels: Vec<String>,
        /// Prefer hosts already running VMs with this tag. Repeat to match a tag set.
        #[arg(long = "affinity-tag")]
        affinity_tags: Vec<String>,
        /// Exclude hosts already running VMs with this tag. Repeat to add more tags.
        #[arg(long = "anti-affinity-tag")]
        anti_affinity_tags: Vec<String>,
        /// Prefer hosts with fewer VMs carrying this tag. Repeat to match a tag set.
        #[arg(long = "spread-tag")]
        spread_tags: Vec<String>,
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
    /// Check whether an OCI image is likely to boot on a real Qarax host/backend
    Preflight {
        /// OCI image reference
        #[arg(long)]
        image_ref: String,
        /// Optional host name or ID to force preflight on
        #[arg(long)]
        host: Option<String>,
        /// Optional target architecture
        #[arg(long)]
        architecture: Option<String>,
        /// Boot mode: 'kernel' (direct Linux boot, default) or 'firmware' (UEFI/EDK2).
        #[arg(long, default_value = "kernel")]
        boot_mode: String,
    },
    /// Stop a VM
    Stop {
        /// VM name or ID
        vm: String,
        /// Block until the VM successfully stops
        #[arg(short, long)]
        wait: bool,
    },
    /// Force stop (hard power-off) a VM
    ForceStop {
        /// VM name or ID
        vm: String,
        /// Block until the VM successfully stops
        #[arg(short, long)]
        wait: bool,
    },
    /// Pause a VM
    Pause {
        /// VM name or ID
        vm: String,
        /// Block until the VM successfully pauses
        #[arg(short, long)]
        wait: bool,
    },
    /// Resume a paused VM
    Resume {
        /// VM name or ID
        vm: String,
        /// Block until the VM successfully resumes
        #[arg(short, long)]
        wait: bool,
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
    /// Attach a storage object as a disk on a VM (local, NFS, or OverlayBD)
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
        /// Enable TCP Segmentation Offload for the NIC
        #[arg(long)]
        offload_tso: Option<bool>,
        /// Enable UDP Fragmentation Offload for the NIC
        #[arg(long)]
        offload_ufo: Option<bool>,
        /// Enable checksum offload for the NIC
        #[arg(long)]
        offload_csum: Option<bool>,
    },
    /// Remove a NIC from a VM (hotunplugs if VM is running)
    RemoveNic {
        /// VM name or ID
        vm: String,
        /// NIC device ID to remove (e.g. "net0")
        #[arg(long)]
        device_id: String,
    },
    /// List security groups bound to a VM
    ListSecurityGroups {
        /// VM name or ID
        vm: String,
    },
    /// Bind a security group to a VM
    AttachSecurityGroup {
        /// VM name or ID
        vm: String,
        /// Security group name or ID
        #[arg(long)]
        security_group: String,
    },
    /// Remove a security group from a VM
    DetachSecurityGroup {
        /// VM name or ID
        vm: String,
        /// Security group name or ID
        #[arg(long)]
        security_group: String,
    },
    /// Resize a disk attached to a stopped VM
    ResizeDisk {
        /// VM name or ID
        vm: String,
        /// Logical disk name (e.g. "rootfs" or "disk0")
        #[arg(long)]
        disk: String,
        /// New size in bytes (must be larger than current size and a multiple of 1 MiB)
        #[arg(long)]
        size: i64,
    },
    /// Resize vCPUs and/or memory of a running VM (hotplug)
    Resize {
        /// VM name or ID
        vm: String,
        /// Target vCPU count (must be within [boot_vcpus, max_vcpus])
        #[arg(long)]
        vcpus: Option<i32>,
        /// Target memory size (e.g. 4GiB; must be within [memory_size, memory_size + hotplug_size])
        #[arg(long, value_parser = super::parse_size)]
        ram: Option<i64>,
    },
    /// Live-migrate a running VM to another host (NFS-backed storage only)
    Migrate {
        /// VM name or ID
        vm: String,
        /// Destination host name or ID
        #[arg(long)]
        host: String,
    },
    /// Convert an OCI image VM to a standalone raw disk (like docker commit)
    Commit {
        /// VM name or ID
        vm: String,
        /// Storage pool name or ID for the committed raw disk (Local or NFS)
        #[arg(long)]
        storage_pool: String,
        /// Size of the committed disk in bytes
        #[arg(long)]
        size: i64,
    },
    /// Manage VM snapshots
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
}

fn build_placement_policy(
    reservation_class: Option<String>,
    required_host_labels: &[String],
    preferred_host_labels: &[String],
    affinity_tags: &[String],
    anti_affinity_tags: &[String],
    spread_tags: &[String],
) -> anyhow::Result<Option<serde_json::Value>> {
    let required_host_labels = parse_key_value_pairs(required_host_labels)?;
    let preferred_host_labels = parse_key_value_pairs(preferred_host_labels)?;
    if reservation_class.is_none()
        && required_host_labels.is_empty()
        && preferred_host_labels.is_empty()
        && affinity_tags.is_empty()
        && anti_affinity_tags.is_empty()
        && spread_tags.is_empty()
    {
        return Ok(None);
    }

    Ok(Some(serde_json::json!({
        "reservation_class": reservation_class,
        "required_host_labels": required_host_labels,
        "preferred_host_labels": preferred_host_labels,
        "affinity_tags": affinity_tags,
        "anti_affinity_tags": anti_affinity_tags,
        "spread_tags": spread_tags,
    })))
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
        /// Snapshot name or ID to restore from
        #[arg(long)]
        snapshot: String,
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
    #[tabled(rename = "Tags")]
    tags: String,
}

pub async fn run(args: VmArgs, client: &Client, output: OutputFormat) -> anyhow::Result<()> {
    match args.command {
        VmCommand::List { tags } => {
            let vms = api::vms::list(client, None, &tags).await?;
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
                        tags: if vm.tags.is_empty() {
                            "-".to_string()
                        } else {
                            vm.tags.join(",")
                        },
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
                if !vm.tags.is_empty() {
                    println!("Tags:        {}", vm.tags.join(", "));
                }
                let nics = api::vms::list_nics(client, id).await.unwrap_or_default();
                for nic in &nics {
                    let ip = nic.ip_address.as_deref().unwrap_or("-");
                    println!("NIC:         {} ip={}", nic.device_id, ip);
                }
            }
        }

        VmCommand::Create {
            name,
            tags,
            vcpus,
            max_vcpus,
            memory,
            template,
            instance_type,
            hypervisor,
            architecture,
            boot_source,
            root_disk,
            description,
            image_ref,
            boot_mode,
            network,
            ip,
            security_groups,
            offload_tso,
            offload_ufo,
            offload_csum,
            cloud_init_user_data,
            cloud_init_meta_data,
            cloud_init_network_config,
            gpu_count,
            gpu_vendor,
            gpu_model,
            min_vram,
            numa_node,
            persistent_upper_pool,
            reservation_class,
            required_host_labels,
            preferred_host_labels,
            affinity_tags,
            anti_affinity_tags,
            spread_tags,
        } => {
            let vm_template_id = match template {
                Some(ref template) => Some(resolve_vm_template_id(client, template).await?),
                None => None,
            };
            let instance_type_id = match instance_type {
                Some(ref instance_type) => {
                    Some(resolve_instance_type_id(client, instance_type).await?)
                }
                None => None,
            };
            let boot_source_id = match boot_source {
                Some(ref s) => Some(resolve_boot_source_id(client, s).await?),
                None => None,
            };
            let root_disk_object_id = match root_disk {
                Some(ref s) => Some(resolve_object_id(client, s).await?),
                None => None,
            };
            let boot_mode_opt = if boot_mode == "kernel" {
                None
            } else {
                Some(boot_mode)
            };
            // When --ip is given we pass an explicit networks entry so the server
            // uses that IP instead of auto-allocating one.
            let explicit_nic = ip.is_some()
                || offload_tso.is_some()
                || offload_ufo.is_some()
                || offload_csum.is_some();
            let (network_id, networks) = match network {
                None => (None, None),
                Some(ref s) => {
                    let nid = resolve_network_id(client, s).await?;
                    if explicit_nic {
                        let iface = NewVmNetwork {
                            id: "net0".to_string(),
                            network_id: Some(nid),
                            ip,
                            offload_tso,
                            offload_ufo,
                            offload_csum,
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
            let hypervisor = hypervisor.or_else(|| {
                if vm_template_id.is_none() {
                    Some("cloud_hv".to_string())
                } else {
                    None
                }
            });
            let max_vcpus = match (max_vcpus, vcpus) {
                (Some(max_vcpus), _) => Some(max_vcpus),
                (None, Some(vcpus)) => Some(vcpus),
                (None, None) => None,
            };

            let accelerator_config =
                build_accelerator_config(gpu_count, &gpu_vendor, &gpu_model, min_vram);

            let numa_config = numa_node.map(|n| serde_json::json!({ "numa_node": n }));

            let persistent_upper_pool_id = match persistent_upper_pool {
                Some(ref p) => Some(resolve_pool_id(client, p).await?),
                None => None,
            };
            let placement_policy = build_placement_policy(
                reservation_class,
                &required_host_labels,
                &preferred_host_labels,
                &affinity_tags,
                &anti_affinity_tags,
                &spread_tags,
            )?;
            let security_group_ids = if security_groups.is_empty() {
                None
            } else {
                let mut ids = Vec::with_capacity(security_groups.len());
                for group in &security_groups {
                    ids.push(resolve_security_group_id(client, group).await?);
                }
                Some(ids)
            };

            let new_vm = NewVm {
                name,
                tags: (!tags.is_empty()).then_some(tags),
                vm_template_id,
                instance_type_id,
                hypervisor,
                architecture,
                boot_vcpus: vcpus,
                max_vcpus,
                memory_size: memory,
                boot_source_id,
                root_disk_object_id,
                boot_mode: boot_mode_opt,
                description,
                image_ref: image_ref.clone(),
                network_id,
                networks,
                security_group_ids,
                cloud_init_user_data: ci_user_data,
                cloud_init_meta_data: ci_meta_data,
                cloud_init_network_config: ci_network_config,
                config: Some(serde_json::json!({})),
                accelerator_config,
                numa_config,
                persistent_upper_pool_id,
                placement_policy,
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

        VmCommand::Preflight {
            image_ref,
            host,
            architecture,
            boot_mode,
        } => {
            let host_id = match host {
                Some(ref host) => Some(resolve_host_id(client, host).await?),
                None => None,
            };
            let request = VmImagePreflightRequest {
                image_ref,
                host_id,
                architecture,
                boot_mode: if boot_mode == "kernel" {
                    None
                } else {
                    Some(boot_mode)
                },
            };
            let response = api::vms::preflight_image(client, &request).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&response, output)?;
            } else {
                println!("Bootable:   {}", response.bootable);
                println!("Host:       {} ({})", response.host_name, response.host_id);
                println!("Resolved:   {}", response.resolved_image_ref);
                println!("Arch:       {}", response.architecture);
                println!();
                for check in response.checks {
                    let status = if check.ok { "ok" } else { "fail" };
                    println!("[{status}] {}: {}", check.name, check.detail);
                }
            }
        }

        VmCommand::Stop { vm, wait } => {
            let id = resolve_vm_id(client, &vm).await?;
            api::vms::stop(client, id).await?;
            if wait {
                crate::wait::wait_for_vm_status(client, id, "shutdown").await?;
            } else {
                println!("Stopped VM: {vm}");
            }
        }

        VmCommand::ForceStop { vm, wait } => {
            let id = resolve_vm_id(client, &vm).await?;
            api::vms::force_stop(client, id).await?;
            if wait {
                crate::wait::wait_for_vm_status(client, id, "shutdown").await?;
            } else {
                println!("Force stopped VM: {vm}");
            }
        }

        VmCommand::Pause { vm, wait } => {
            let id = resolve_vm_id(client, &vm).await?;
            api::vms::pause(client, id).await?;
            if wait {
                crate::wait::wait_for_vm_status(client, id, "paused").await?;
            } else {
                println!("Paused VM: {vm}");
            }
        }

        VmCommand::Resume { vm, wait } => {
            let id = resolve_vm_id(client, &vm).await?;
            api::vms::resume(client, id).await?;
            if wait {
                crate::wait::wait_for_vm_status(client, id, "running").await?;
            } else {
                println!("Resumed VM: {vm}");
            }
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
            offload_tso,
            offload_ufo,
            offload_csum,
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
                offload_tso,
                offload_ufo,
                offload_csum,
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

        VmCommand::ListSecurityGroups { vm } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            let groups = api::vms::list_security_groups(client, vm_id).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&groups, output)?;
            } else if groups.is_empty() {
                println!("No security groups bound to VM {vm}");
            } else {
                for group in groups {
                    println!(
                        "{}\t{}\t{}",
                        group.id,
                        group.name,
                        group.description.unwrap_or_else(|| "-".to_string())
                    );
                }
            }
        }

        VmCommand::AttachSecurityGroup { vm, security_group } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            let security_group_id = resolve_security_group_id(client, &security_group).await?;
            api::security_groups::attach_to_vm(client, vm_id, security_group_id).await?;
            println!("Attached security group {security_group} to VM {vm}");
        }

        VmCommand::DetachSecurityGroup { vm, security_group } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            let security_group_id = resolve_security_group_id(client, &security_group).await?;
            api::security_groups::detach_from_vm(client, vm_id, security_group_id).await?;
            println!("Detached security group {security_group} from VM {vm}");
        }

        VmCommand::ResizeDisk { vm, disk, size } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            let req = DiskResizeRequest {
                new_size_bytes: size,
            };
            let updated = api::vms::resize_disk(client, vm_id, &disk, &req).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&updated, output)?;
            } else {
                println!(
                    "Resized disk {disk} on VM {vm}: new size = {}",
                    format_bytes(updated.size_bytes)
                );
            }
        }

        VmCommand::Resize { vm, vcpus, ram } => {
            if vcpus.is_none() && ram.is_none() {
                return Err(anyhow!("At least one of --vcpus or --ram must be provided"));
            }
            let vm_id = resolve_vm_id(client, &vm).await?;
            let req = VmResizeRequest {
                desired_vcpus: vcpus,
                desired_ram: ram,
            };
            let updated = api::vms::resize(client, vm_id, &req).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&updated, output)?;
            } else {
                println!(
                    "Resized VM {vm}: vcpus={}, memory={}",
                    updated.boot_vcpus, updated.memory_size
                );
            }
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

        VmCommand::Commit {
            vm,
            storage_pool,
            size,
        } => {
            let vm_id = resolve_vm_id(client, &vm).await?;
            let pool_id = resolve_pool_id(client, &storage_pool).await?;
            let req = CommitVmRequest {
                storage_pool_id: pool_id,
                size_bytes: size,
            };
            let resp = api::vms::commit(client, vm_id, &req).await?;
            if !matches!(output, OutputFormat::Table) {
                print_output(&resp, output)?;
            } else {
                println!("Committing VM: {vm}");
                println!("Job:           {}", resp.job_id);
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
                let snapshots = api::vms::list_snapshots(client, id, None).await?;
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
                let snapshot_id = super::resolve_snapshot_id(client, id, &snapshot).await?;
                let req = RestoreRequest { snapshot_id };
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

/// Redraw the progress bar in-place on stderr.
fn draw_bar(stderr: &mut impl std::io::Write, pct: usize, desc: &str, elapsed: u64) {
    use crossterm::{
        cursor,
        terminal::{self, ClearType},
    };
    const BAR_WIDTH: usize = 30;
    let filled = BAR_WIDTH * pct / 100;
    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(BAR_WIDTH - filled));
    let _ = crossterm::execute!(
        stderr,
        cursor::MoveToColumn(0),
        terminal::Clear(ClearType::CurrentLine),
    );
    let _ = write!(stderr, "  {} {:>3}%  {}  {}s", bar, pct, desc, elapsed);
    let _ = stderr.flush();
}

/// Poll a job until it completes or fails, animating the progress bar between
/// polled values so it never jumps ahead suddenly.
async fn poll_job(client: &Client, job_id: Uuid) -> anyhow::Result<()> {
    use crossterm::{
        cursor,
        terminal::{self, ClearType},
    };
    use std::time::Instant;

    const POLL_INTERVAL_MS: u64 = 2000;
    const FRAME_MS: u64 = 50;

    let started = Instant::now();
    let mut stderr = std::io::stderr();
    let mut displayed_pct: usize = 0;

    loop {
        let job = api::jobs::get(client, job_id).await?;
        let elapsed = started.elapsed().as_secs();

        match job.status.as_str() {
            "completed" => {
                // Animate to 100% before finishing.
                let desc = job.description.as_deref().unwrap_or("completing");
                while displayed_pct < 100 {
                    displayed_pct += 1;
                    draw_bar(
                        &mut stderr,
                        displayed_pct,
                        desc,
                        started.elapsed().as_secs(),
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(FRAME_MS / 2)).await;
                }
                let _ = crossterm::execute!(
                    stderr,
                    cursor::MoveToColumn(0),
                    terminal::Clear(ClearType::CurrentLine),
                );
                eprintln!("  done in {}s", started.elapsed().as_secs());
                return Ok(());
            }
            "failed" => {
                let _ = crossterm::execute!(
                    stderr,
                    cursor::MoveToColumn(0),
                    terminal::Clear(ClearType::CurrentLine),
                );
                return Err(anyhow!(
                    "Job {job_id} failed: {}",
                    job.error.unwrap_or_else(|| "unknown error".to_string())
                ));
            }
            status => {
                let target_pct = job.progress.unwrap_or(0).clamp(0, 99) as usize;
                let desc = job.description.as_deref().unwrap_or(status).to_string();

                // Animate from current displayed percentage toward target over the poll interval.
                let frames = POLL_INTERVAL_MS / FRAME_MS;
                let start_pct = displayed_pct;
                for frame in 0..frames {
                    // Interpolate linearly toward target.
                    let interp = start_pct
                        + (target_pct.saturating_sub(start_pct)) * (frame as usize + 1)
                            / frames as usize;
                    if interp > displayed_pct {
                        displayed_pct = interp;
                    }
                    draw_bar(
                        &mut stderr,
                        displayed_pct,
                        &desc,
                        started.elapsed().as_secs(),
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(FRAME_MS)).await;
                }

                let _ = elapsed; // already used via started.elapsed() inside draw_bar calls
            }
        }
    }
}
