use std::collections::HashMap;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::{Extension, Json, extract::Path, response::IntoResponse};
use futures::{SinkExt, StreamExt};
use http::StatusCode;
#[cfg(feature = "otel")]
use opentelemetry::KeyValue;
use serde::{Deserialize, Serialize};
#[cfg(feature = "otel")]
use std::time::Instant;
use tracing::{error, info, instrument, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use common::cpu_list::expand_cpu_list;

use crate::{
    App,
    grpc_client::{
        CreateVmRequest, NodeClient, net_configs_from_db,
        node::{
            CpuPinning, DiskConfig, FsConfig, NetConfig, NumaPlacement, VhostMode, VsockConfig,
        },
    },
    model::{
        boot_sources, host_gpus, host_numa, hosts,
        hosts::Host,
        jobs::{self, JobType, NewJob},
        lifecycle_hooks,
        network_interfaces::{self, NetworkInterface},
        networks,
        sandboxes::{self, SandboxStatus},
        snapshots,
        snapshots::{NewSnapshot, Snapshot, SnapshotStatus},
        storage_objects::{self, NewStorageObject, StorageObjectType},
        storage_pools::{self, OverlayBdPoolConfig},
        vm_disks::{self, NewVmDisk},
        vm_filesystems::{self, NewVmFilesystem},
        vm_templates::{self, CreateVmTemplateFromVmRequest},
        vms::{self, BootMode, NewVm, NewVmNetwork, ResolvedNewVm, Vm, VmStatus},
    },
};

use super::{ApiResponse, Result};

#[derive(Serialize, ToSchema)]
pub struct CreateVmResponse {
    pub vm_id: Uuid,
    pub job_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct VmStartResponse {
    pub job_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct VmMetrics {
    pub vm_id: Uuid,
    pub status: VmStatus,
    pub memory_actual_size: Option<i64>,
    pub counters: HashMap<String, HashMap<String, i64>>,
}

fn resolved_vm_architecture(env: &App, architecture: Option<&str>) -> String {
    architecture
        .and_then(common::architecture::normalize_architecture)
        .unwrap_or_else(|| env.control_plane_architecture().to_string())
}

fn gpu_request(accel: Option<&host_gpus::AcceleratorConfig>) -> Option<hosts::GpuRequest> {
    accel.map(|accel| hosts::GpuRequest {
        count: accel.gpu_count,
        vendor: accel.gpu_vendor.clone(),
        model: accel.gpu_model.clone(),
        min_vram_bytes: accel.min_vram_bytes,
    })
}

fn persisted_vm_architecture(vm: &Vm) -> Option<String> {
    vm.config
        .get("architecture")
        .and_then(|value| value.as_str())
        .and_then(common::architecture::normalize_architecture)
}

fn ensure_host_matches_architecture(host: &Host, architecture: &str) -> Result<()> {
    if let Some(host_architecture) = host.architecture.as_deref()
        && host_architecture != architecture
    {
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "selected host architecture {} does not match VM architecture {}",
            host_architecture, architecture
        )));
    }

    Ok(())
}

async fn root_disk_scheduling_requirements(
    env: &App,
    root_disk_object_id: Option<Uuid>,
) -> Result<(i64, Option<Uuid>)> {
    let Some(object_id) = root_disk_object_id else {
        return Ok((0, None));
    };

    let object = storage_objects::get(env.pool(), object_id).await?;
    match object.object_type {
        StorageObjectType::Disk | StorageObjectType::Snapshot | StorageObjectType::OciImage => {}
        _ => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "root_disk_object_id must reference a disk-like storage object".into(),
            ));
        }
    }

    let pool = storage_pools::get(env.pool(), object.storage_pool_id).await?;
    let storage_pool_id = match pool.pool_type {
        storage_pools::StoragePoolType::Local | storage_pools::StoragePoolType::Nfs => {
            Some(pool.id)
        }
        storage_pools::StoragePoolType::OverlayBd => None,
    };

    Ok((object.size_bytes.max(0), storage_pool_id))
}

async fn validate_root_disk_for_host(
    env: &App,
    host_id: Uuid,
    object_id: Uuid,
    maybe_pool_id: Option<Uuid>,
) -> Result<()> {
    let Some(pool_id) = maybe_pool_id else {
        return Ok(());
    };

    if !storage_pools::host_has_pool(env.pool(), host_id, pool_id).await? {
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "selected host is not attached to the storage pool backing root_disk_object_id {}",
            object_id
        )));
    }

    Ok(())
}

async fn scheduling_request_for_vm(
    env: &App,
    vm: &ResolvedNewVm,
    accel: Option<&host_gpus::AcceleratorConfig>,
) -> Result<hosts::SchedulingRequest> {
    let (disk_bytes, storage_pool_id) =
        root_disk_scheduling_requirements(env, vm.root_disk_object_id).await?;
    Ok(hosts::SchedulingRequest {
        memory_bytes: vm.memory_size,
        vcpus: vm.boot_vcpus,
        disk_bytes,
        architecture: Some(resolved_vm_architecture(env, vm.architecture.as_deref())),
        storage_pool_id,
        gpu: gpu_request(accel),
    })
}

async fn pick_host(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    env: &App,
    request: &hosts::SchedulingRequest,
) -> Result<Host> {
    let host = hosts::pick_host_tx(tx, request, env.scheduling()).await?;

    host.inspect(|host| {
        info!(
            host_id = %host.id,
            host_name = %host.name,
            requested_memory_bytes = request.memory_bytes,
            requested_vcpus = request.vcpus,
            requested_disk_bytes = request.disk_bytes,
            architecture = ?request.architecture,
            has_gpu_request = request.gpu.is_some(),
            "scheduler selected host"
        );
    })
    .ok_or_else(|| {
        warn!(
            requested_memory_bytes = request.memory_bytes,
            requested_vcpus = request.vcpus,
            requested_disk_bytes = request.disk_bytes,
            architecture = ?request.architecture,
            has_gpu_request = request.gpu.is_some(),
            "scheduler found no eligible host"
        );
        crate::errors::Error::UnprocessableEntity(
            "no hosts in UP state with sufficient resources available for scheduling".into(),
        )
    })
}

fn persist_vm_scheduling_metadata(vm: &mut ResolvedNewVm, architecture: &str) {
    if let serde_json::Value::Object(map) = &mut vm.config {
        map.insert(
            "architecture".to_string(),
            serde_json::Value::String(architecture.to_string()),
        );
        if let Some(nc) = vm.numa_config.clone() {
            map.insert("numa_config".to_string(), nc);
        }
    }
}

/// Allocate GPUs for a VM inside an existing transaction. Returns an error if
/// fewer GPUs were allocated than requested.
async fn allocate_vm_gpus(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    host_id: Uuid,
    vm_id: Uuid,
    accel: &host_gpus::AcceleratorConfig,
) -> Result<()> {
    let allocated = host_gpus::allocate_gpus(
        tx,
        host_id,
        vm_id,
        accel.gpu_count,
        accel.gpu_vendor.as_deref(),
        accel.gpu_model.as_deref(),
        accel.min_vram_bytes,
    )
    .await?;
    if (allocated.len() as i32) < accel.gpu_count {
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "requested {} GPUs but only {} available on selected host",
            accel.gpu_count,
            allocated.len()
        )));
    }
    Ok(())
}

/// Derive the rootfs path for an OCI image the same way the image store does:
/// replace '/', ':', '@' with '_' and place under /var/lib/qarax/images/<safe>\/rootfs.
/// Must stay in sync with `qarax-node/src/image_store/manager.rs::safe_name()`.
fn image_ref_to_rootfs_path(image_ref: &str) -> String {
    let safe: String = image_ref
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '@') { '_' } else { c })
        .collect();
    format!("/var/lib/qarax/images/{}/rootfs", safe)
}

async fn create_root_disk_record(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    vm_id: Uuid,
    root_disk_object_id: Uuid,
) -> Result<()> {
    let disk = NewVmDisk {
        vm_id,
        storage_object_id: Some(root_disk_object_id),
        logical_name: "rootfs".to_string(),
        device_path: "/dev/vda".to_string(),
        boot_order: Some(0),
        read_only: Some(false),
        config: serde_json::json!({}),
        ..Default::default()
    };
    vm_disks::create_tx(tx, &disk).await?;
    Ok(())
}

/// Resolve the host that a VM is assigned to, for routing subsequent operations.
async fn host_for_vm(env: &App, vm_id: Uuid) -> Result<Host> {
    let vm = vms::get(env.pool(), vm_id).await?;
    let host_id = vm.host_id.ok_or_else(|| {
        crate::errors::Error::UnprocessableEntity("VM has no assigned host".into())
    })?;
    hosts::get_by_id(env.pool(), host_id)
        .await?
        .ok_or_else(|| crate::errors::Error::UnprocessableEntity("assigned host not found".into()))
}

async fn ensure_resize_capacity(
    env: &App,
    vm: &Vm,
    host: &Host,
    desired_vcpus: Option<i32>,
    desired_ram: Option<i64>,
) -> Result<()> {
    let Some(capacity) = hosts::get_resource_capacity(env.pool(), host.id).await? else {
        return Err(crate::errors::Error::NotFound);
    };

    let target_vcpus = desired_vcpus.unwrap_or(vm.boot_vcpus) as i64;
    if let Some(total_cpus) = capacity.total_cpus {
        let allocated_vcpus = capacity.allocated_vcpus - i64::from(vm.boot_vcpus) + target_vcpus;
        let max_vcpus =
            (f64::from(total_cpus) * env.scheduling().cpu_oversubscription_ratio).floor();
        if (allocated_vcpus as f64) > max_vcpus {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "resizing to {} vCPUs would exceed host CPU capacity on {}",
                target_vcpus, host.name
            )));
        }
    }

    let target_memory = desired_ram.unwrap_or(vm.memory_size);
    if let Some(total_memory_bytes) = capacity.total_memory_bytes {
        let allocated_memory = capacity.allocated_memory_bytes - vm.memory_size + target_memory;
        let max_memory =
            (total_memory_bytes as f64 * env.scheduling().memory_oversubscription_ratio).floor();
        if (allocated_memory as f64) > max_memory {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "resizing to {} bytes would exceed host memory capacity on {}",
                target_memory, host.name
            )));
        }
    }

    if desired_ram.is_some_and(|ram| ram > vm.memory_size)
        && let Some(available_memory_bytes) = capacity.available_memory_bytes
        && available_memory_bytes <= env.scheduling().memory_health_floor_bytes
    {
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "host {} is below the configured memory health floor",
            host.name
        )));
    }

    Ok(())
}

#[utoipa::path(
    get,
    path = "/vms",
    params(crate::handlers::VmListQuery),
    responses(
        (status = 200, description = "List all VMs", body = Vec<Vm>),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::VmListQuery>,
) -> Result<ApiResponse<Vec<Vm>>> {
    let tags: Vec<String> = query
        .tags
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let vms = vms::list(env.pool(), query.name.as_deref(), &tags).await?;
    Ok(ApiResponse {
        data: vms,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/vms/{vm_id}",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM details", body = Vm),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<Vm>> {
    let vm = vms::get(env.pool(), vm_id).await?;
    Ok(ApiResponse {
        data: vm,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/vms",
    request_body = NewVm,
    responses(
        (status = 201, description = "VM created successfully (synchronous)", body = String, content_type = "application/json"),
        (status = 202, description = "VM creation started asynchronously", body = CreateVmResponse),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(vm): Json<NewVm>,
) -> Result<axum::response::Response> {
    let vm = vms::resolve_create_request(env.pool(), vm).await?;

    // If an OCI image_ref is provided, use the async job path
    if vm.image_ref.is_some() {
        return create_with_image(env, vm).await;
    }

    let id = create_vm_internal(&env, vm).await?;
    Ok(ApiResponse {
        data: id.to_string(),
        code: StatusCode::CREATED,
    }
    .into_response())
}

/// Create a VM from a resolved config: pick host, write DB records, allocate network.
/// Does NOT contact the node. Node provisioning is deferred to VM start.
pub(crate) async fn create_vm_internal(env: &App, vm: ResolvedNewVm) -> Result<Uuid> {
    let accel_config = vm
        .accelerator_config
        .as_ref()
        .and_then(host_gpus::AcceleratorConfig::from_value);
    let mut vm = vm;
    let scheduling_request = scheduling_request_for_vm(env, &vm, accel_config.as_ref()).await?;
    let target_architecture = scheduling_request
        .architecture
        .clone()
        .unwrap_or_else(|| env.control_plane_architecture().to_string());
    persist_vm_scheduling_metadata(&mut vm, &target_architecture);

    let mut tx = env.pool().begin().await?;
    let host = pick_host(&mut tx, env, &scheduling_request).await?;
    ensure_host_matches_architecture(&host, &target_architecture)?;
    if let Some(root_disk_object_id) = vm.root_disk_object_id {
        validate_root_disk_for_host(
            env,
            host.id,
            root_disk_object_id,
            scheduling_request.storage_pool_id,
        )
        .await?;
    }

    let id = vms::create_tx(&mut tx, &vm, Some(host.id)).await?;
    if let Some(root_disk_object_id) = vm.root_disk_object_id {
        create_root_disk_record(&mut tx, id, root_disk_object_id).await?;
    }

    for net in vm.networks.as_deref().unwrap_or(&[]) {
        network_interfaces::create(&mut tx, id, net).await?;
    }

    if let Some(ref accel) = accel_config {
        allocate_vm_gpus(&mut tx, host.id, id, accel).await?;
    }

    tx.commit().await?;

    register_static_ips(env.pool(), id, vm.networks.as_deref()).await?;

    if let Some(network_id) = vm.network_id {
        create_managed_network_interface(env.pool(), id, network_id).await?;
    }

    Ok(id)
}

/// Async path: pull OCI image and create VM in a background job, return 202 immediately.
async fn create_with_image(env: App, vm: ResolvedNewVm) -> Result<axum::response::Response> {
    let image_ref = vm
        .image_ref
        .clone()
        .expect("image_ref checked before calling");

    // Pick host eagerly so we return 422 immediately if none are UP
    let accel_config = vm
        .accelerator_config
        .as_ref()
        .and_then(host_gpus::AcceleratorConfig::from_value);
    let mut vm = vm;
    let scheduling_request = scheduling_request_for_vm(&env, &vm, accel_config.as_ref()).await?;
    let target_architecture = scheduling_request
        .architecture
        .clone()
        .unwrap_or_else(|| env.control_plane_architecture().to_string());
    persist_vm_scheduling_metadata(&mut vm, &target_architecture);

    let mut tx = env.pool().begin().await?;
    let host = pick_host(&mut tx, &env, &scheduling_request).await?;
    ensure_host_matches_architecture(&host, &target_architecture)?;
    if let Some(root_disk_object_id) = vm.root_disk_object_id {
        validate_root_disk_for_host(
            &env,
            host.id,
            root_disk_object_id,
            scheduling_request.storage_pool_id,
        )
        .await?;
    }

    // Check if the selected host has an OverlayBD storage pool; if so, use that path.
    let overlaybd_pool = storage_pools::find_overlaybd_for_host(env.pool(), host.id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to query OverlayBD pool for host {}: {}", host.id, e);
            crate::errors::Error::InternalServerError
        })?;

    // Resolve boot info based on boot_mode (stored for use at start time)
    let boot_mode = vm.boot_mode.clone().unwrap_or(BootMode::Kernel);
    match boot_mode {
        BootMode::Kernel => {
            let (_kernel, _initramfs, _cmdline) = if let Some(boot_source_id) = vm.boot_source_id {
                let resolved = boot_sources::resolve(env.pool(), boot_source_id)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            "Failed to resolve boot_source_id {}: {}",
                            boot_source_id,
                            e
                        );
                        crate::errors::Error::UnprocessableEntity(format!(
                            "Invalid boot_source_id: {}",
                            boot_source_id
                        ))
                    })?;
                (
                    resolved.kernel_path,
                    resolved.initramfs_path,
                    resolved.kernel_params,
                )
            } else {
                let d = env.vm_defaults();
                (
                    d.kernel.as_ref().to_string(),
                    d.initramfs
                        .as_ref()
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string()),
                    d.cmdline.as_ref().to_string(),
                )
            };
        }
        BootMode::Firmware => {
            let d = env.vm_defaults();
            let firmware = d
                .firmware
                .as_ref()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            if firmware.is_none() {
                return Err(crate::errors::Error::UnprocessableEntity(
                    "firmware boot mode requires a firmware path (set vm_defaults.firmware or VM_FIRMWARE env var)".into(),
                ));
            }
        }
    }

    // Validate persistent_upper_pool_id if provided: must be Local/NFS, Active,
    // and already attached to the selected host.
    let persistent_upper_pool_id = vm.persistent_upper_pool_id;
    if let Some(upper_pool_id) = persistent_upper_pool_id {
        let upper_pool = storage_pools::get(env.pool(), upper_pool_id)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => crate::errors::Error::UnprocessableEntity(format!(
                    "persistent_upper_pool_id {} not found",
                    upper_pool_id
                )),
                _ => {
                    tracing::error!("Failed to get storage pool {}: {}", upper_pool_id, e);
                    crate::errors::Error::InternalServerError
                }
            })?;

        if upper_pool.status != storage_pools::StoragePoolStatus::Active {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "persistent_upper_pool_id {} is not active",
                upper_pool_id
            )));
        }

        match upper_pool.pool_type {
            storage_pools::StoragePoolType::Local | storage_pools::StoragePoolType::Nfs => {}
            _ => {
                return Err(crate::errors::Error::UnprocessableEntity(
                    "persistent_upper_pool_id must be a Local or NFS pool".into(),
                ));
            }
        }

        let attached = storage_pools::host_has_pool(env.pool(), host.id, upper_pool_id)
            .await
            .map_err(|e| {
                tracing::error!("Failed to check pool attachment: {}", e);
                crate::errors::Error::InternalServerError
            })?;
        if !attached {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "persistent_upper_pool_id {} is not attached to the selected host",
                upper_pool_id
            )));
        }
    }

    // Create VM row with PENDING status
    let vm_id = vms::create_tx_with_status(&mut tx, &vm, Some(host.id), VmStatus::Pending).await?;

    // Store network interfaces in the transaction
    for net in vm.networks.as_deref().unwrap_or(&[]) {
        network_interfaces::create(&mut tx, vm_id, net).await?;
    }
    if let Some(root_disk_object_id) = vm.root_disk_object_id {
        create_root_disk_record(&mut tx, vm_id, root_disk_object_id).await?;
    }

    // Allocate GPUs atomically inside the transaction
    if let Some(ref accel) = accel_config {
        allocate_vm_gpus(&mut tx, host.id, vm_id, accel).await?;
    }

    tx.commit().await?;

    // For any explicit networks with a static IP + network_id, register in IPAM.
    register_static_ips(env.pool(), vm_id, vm.networks.as_deref()).await?;

    // If network_id is provided, allocate an IP and create a default interface.
    // This mirrors the synchronous create path so async image-based VMs get managed networking.
    if let Some(network_id) = vm.network_id {
        create_managed_network_interface(env.pool(), vm_id, network_id).await?;
    }

    // Create job record
    let job = jobs::create(
        env.pool(),
        NewJob {
            job_type: JobType::ImagePull,
            description: Some(format!("Pulling image {}", image_ref)),
            resource_id: Some(vm_id),
            resource_type: Some("vm".to_string()),
        },
    )
    .await?;
    let job_id = job.id;

    // Spawn background task
    let db_pool = env.pool_arc();

    tokio::spawn(async move {
        tracing::info!(vm_id = %vm_id, job_id = %job_id, image_ref = %image_ref, "Starting async OCI image pull");

        if let Err(e) = jobs::mark_running(&db_pool, job_id).await {
            tracing::error!(job_id = %job_id, error = %e, "Failed to mark job as running");
            return;
        }

        let node_client = NodeClient::new(&host.address, host.port as u16);

        if let Some(pool) = overlaybd_pool {
            // OverlayBD path: import image to local registry, then boot via virtio-blk
            run_overlaybd_create(
                &node_client,
                &db_pool,
                vm_id,
                job_id,
                &image_ref,
                &pool.config,
                pool.id,
                persistent_upper_pool_id,
            )
            .await;
        } else {
            // Virtiofs path: pull image via Nydus, serve rootfs via virtiofs
            run_virtiofs_create(&node_client, &db_pool, vm_id, job_id, &image_ref).await;
        }
    });

    use axum::response::IntoResponse as _;
    Ok(ApiResponse {
        data: CreateVmResponse { vm_id, job_id },
        code: StatusCode::ACCEPTED,
    }
    .into_response())
}

/// Allocate an IP from the given network and create the default network interface record for a VM.
async fn create_managed_network_interface(
    pool: &sqlx::PgPool,
    vm_id: uuid::Uuid,
    network_id: uuid::Uuid,
) -> Result<(), crate::errors::Error> {
    let ip = networks::next_available_ip(pool, network_id)
        .await?
        .ok_or_else(|| {
            crate::errors::Error::UnprocessableEntity("No available IPs in network".into())
        })?;
    networks::allocate_ip(pool, network_id, &ip, Some(vm_id)).await?;
    let net = crate::model::vms::NewVmNetwork {
        id: "net0".to_string(),
        network_id: Some(network_id),
        ip: Some(ip),
        ..Default::default()
    };
    let mut tx = pool.begin().await?;
    network_interfaces::create(&mut tx, vm_id, &net).await?;
    tx.commit().await?;
    Ok(())
}

/// Helper to register explicitly provided static IPs in IPAM during VM creation.
async fn register_static_ips(
    pool: &sqlx::PgPool,
    vm_id: Uuid,
    networks: Option<&[crate::model::vms::NewVmNetwork]>,
) -> Result<()> {
    for net in networks.unwrap_or(&[]) {
        if let (Some(net_id), Some(ip)) = (net.network_id, &net.ip) {
            networks::allocate_ip(pool, net_id, ip, Some(vm_id)).await?;
        }
    }
    Ok(())
}

/// Background task for the virtiofs (Nydus) OCI image boot path.
async fn run_virtiofs_create(
    node_client: &NodeClient,
    db_pool: &sqlx::PgPool,
    vm_id: uuid::Uuid,
    job_id: uuid::Uuid,
    image_ref: &str,
) {
    // Step 1: Pull image
    let image_info = match node_client.pull_image(image_ref).await {
        Ok(info) => info,
        Err(e) => {
            let msg = format!("Failed to pull OCI image: {}", e);
            tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
            let _ = jobs::mark_failed(db_pool, job_id, &msg).await;
            let _ = vms::update_status(db_pool, vm_id, VmStatus::Unknown).await;
            return;
        }
    };

    let _ = jobs::update_progress(db_pool, job_id, 50).await;

    // Step 2: Persist virtiofs filesystem record
    let fs_record = NewVmFilesystem {
        vm_id,
        tag: "rootfs".to_string(),
        num_queues: Some(1),
        queue_size: Some(1024),
        pci_segment: None,
        image_ref: Some(image_ref.to_string()),
        image_digest: Some(image_info.digest.clone()),
    };
    if let Err(e) = vm_filesystems::create(db_pool, &fs_record).await {
        tracing::warn!(vm_id = %vm_id, error = %e, "Failed to persist filesystem record");
    }

    // Mark VM as created (on node provisioning is deferred to vm start)
    let _ = vms::update_status(db_pool, vm_id, VmStatus::Created).await;
    let result = serde_json::json!({ "digest": image_info.digest });
    let _ = jobs::mark_completed(db_pool, job_id, Some(result)).await;

    tracing::info!(vm_id = %vm_id, job_id = %job_id, "VM creation job completed (virtiofs)");
}

/// Background task for the OverlayBD lazy block loading path.
#[allow(clippy::too_many_arguments)]
async fn run_overlaybd_create(
    node_client: &NodeClient,
    db_pool: &sqlx::PgPool,
    vm_id: uuid::Uuid,
    job_id: uuid::Uuid,
    image_ref: &str,
    pool_config: &serde_json::Value,
    storage_pool_id: uuid::Uuid,
    persistent_upper_pool_id: Option<uuid::Uuid>,
) {
    // Extract registry URL from pool config
    let registry_url = match OverlayBdPoolConfig::from_value(pool_config) {
        Some(cfg) => cfg.url,
        None => {
            let msg = "OverlayBD pool config missing 'url' field".to_string();
            tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
            let _ = jobs::mark_failed(db_pool, job_id, &msg).await;
            let _ = vms::update_status(db_pool, vm_id, VmStatus::Unknown).await;
            return;
        }
    };

    // Step 1: Import (convert + push) image to local registry
    let import_result = match node_client
        .import_overlaybd_image(image_ref, &registry_url)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Failed to import OverlayBD image: {}", e);
            tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
            let _ = jobs::mark_failed(db_pool, job_id, &msg).await;
            let _ = vms::update_status(db_pool, vm_id, VmStatus::Unknown).await;
            return;
        }
    };

    let _ = jobs::update_progress(db_pool, job_id, 50).await;

    // Step 2: Create a storage object for the imported image, then persist a vm_disk record
    let so_config = serde_json::json!({
        "image_ref": import_result.image_ref,
        "registry_url": registry_url,
        "digest": if import_result.digest.is_empty() { None } else { Some(&import_result.digest) },
    });
    let so_id = match storage_objects::create(
        db_pool,
        storage_objects::NewStorageObject {
            name: format!("overlaybd-{}", vm_id),
            storage_pool_id: Some(storage_pool_id),
            object_type: storage_objects::StorageObjectType::OciImage,
            size_bytes: 0,
            config: so_config,
            parent_id: None,
        },
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            let msg = format!("Failed to create storage object for OverlayBD disk: {}", e);
            tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
            let _ = jobs::mark_failed(db_pool, job_id, &msg).await;
            let _ = vms::update_status(db_pool, vm_id, VmStatus::Unknown).await;
            return;
        }
    };

    // Pick the next available vd* disk ID (vda, vdb, vdc, ...)
    let existing_disks = match vm_disks::list_by_vm(db_pool, vm_id).await {
        Ok(d) => d,
        Err(e) => {
            let msg = format!("Failed to list existing disks: {}", e);
            tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
            let _ = jobs::mark_failed(db_pool, job_id, &msg).await;
            let _ = vms::update_status(db_pool, vm_id, VmStatus::Unknown).await;
            return;
        }
    };
    let logical_name = next_disk_id(&existing_disks);

    // Optionally create a persistent OverlaybdUpper StorageObject on the
    // caller-supplied pool so that writes survive VM deletion.
    let upper_so_id = if let Some(upper_pool_id) = persistent_upper_pool_id {
        match storage_objects::create(
            db_pool,
            storage_objects::NewStorageObject {
                name: format!("overlaybd-upper-{}", vm_id),
                storage_pool_id: Some(upper_pool_id),
                object_type: storage_objects::StorageObjectType::OverlaybdUpper,
                size_bytes: 0,
                config: serde_json::Value::Object(serde_json::Map::new()),
                parent_id: None,
            },
        )
        .await
        {
            Ok(id) => Some(id),
            Err(e) => {
                let msg = format!("Failed to create OverlaybdUpper storage object: {}", e);
                tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
                if let Err(cleanup_err) = storage_objects::delete(db_pool, so_id).await {
                    tracing::warn!(vm_id = %vm_id, storage_object_id = %so_id, error = %cleanup_err, "Failed to clean up orphaned OciImage storage object");
                }
                let _ = jobs::mark_failed(db_pool, job_id, &msg).await;
                let _ = vms::update_status(db_pool, vm_id, VmStatus::Unknown).await;
                return;
            }
        }
    } else {
        None
    };

    let disk_record = NewVmDisk {
        vm_id,
        storage_object_id: Some(so_id),
        logical_name: logical_name.clone(),
        device_path: format!("/dev/{}", logical_name),
        boot_order: Some(0),
        read_only: Some(false),
        upper_storage_object_id: upper_so_id,
        ..Default::default()
    };
    if let Err(e) = vm_disks::create(db_pool, &disk_record).await {
        let msg = format!("Failed to persist OverlayBD disk record: {}", e);
        tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
        if let Err(cleanup_err) = storage_objects::delete(db_pool, so_id).await {
            tracing::warn!(vm_id = %vm_id, storage_object_id = %so_id, error = %cleanup_err, "Failed to clean up orphaned storage object");
        }
        if let Some(upper_id) = upper_so_id
            && let Err(cleanup_err) = storage_objects::delete(db_pool, upper_id).await
        {
            tracing::warn!(vm_id = %vm_id, storage_object_id = %upper_id, error = %cleanup_err, "Failed to clean up orphaned OverlaybdUpper storage object");
        }
        let _ = jobs::mark_failed(db_pool, job_id, &msg).await;
        let _ = vms::update_status(db_pool, vm_id, VmStatus::Unknown).await;
        return;
    }

    // Mark VM as created (node provisioning is deferred to vm start)
    let _ = vms::update_status(db_pool, vm_id, VmStatus::Created).await;
    let result = serde_json::json!({ "digest": import_result.digest });
    let _ = jobs::mark_completed(db_pool, job_id, Some(result)).await;

    tracing::info!(vm_id = %vm_id, job_id = %job_id, "VM creation job completed (overlaybd)");
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/start",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 202, description = "VM start accepted", body = VmStartResponse),
        (status = 404, description = "VM not found"),
        (status = 422, description = "VM not in a startable state"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn start(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<axum::response::Response> {
    let job_id = start_vm_internal(&env, vm_id).await?;
    use axum::response::IntoResponse as _;
    Ok(ApiResponse {
        data: VmStartResponse { job_id },
        code: StatusCode::ACCEPTED,
    }
    .into_response())
}

/// Kick off an async VM start: validate state, build CreateVmRequest, spawn background task.
/// Returns the job ID.
pub(crate) async fn start_vm_internal(env: &App, vm_id: Uuid) -> Result<Uuid> {
    let vm = vms::get(env.pool(), vm_id).await?;
    let original_status = vm.status.clone();
    #[cfg(feature = "otel")]
    let metrics = env.metrics_arc();
    #[cfg(feature = "otel")]
    let initial_status_label = original_status.to_string();

    match vm.status {
        VmStatus::Running => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "VM is already running".into(),
            ));
        }
        VmStatus::Paused => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "VM is paused; use POST /vms/{vm_id}/resume instead".into(),
            ));
        }
        VmStatus::Pending => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "VM is pending job completion; wait for the job to finish".into(),
            ));
        }
        VmStatus::Migrating => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "VM is being migrated; wait for migration to finish".into(),
            ));
        }
        VmStatus::Created | VmStatus::Shutdown | VmStatus::Unknown => {
            // Valid states to start from
        }
    }

    let host = host_for_vm(env, vm_id).await?;
    if let Some(architecture) = persisted_vm_architecture(&vm) {
        ensure_host_matches_architecture(&host, &architecture)?;
    }

    // Set VM status to Pending to prevent double-start
    vms::update_status(env.pool(), vm_id, VmStatus::Pending).await?;

    // Build create request eagerly (before spawning) so we can return errors synchronously
    let create_req = if original_status == VmStatus::Created {
        Some(build_create_vm_request(env, &vm).await.map_err(|e| {
            tracing::error!("Failed to build CreateVmRequest for {}: {}", vm_id, e);
            e
        })?)
    } else {
        None
    };

    // Create job record
    let job = jobs::create(
        env.pool(),
        NewJob {
            job_type: JobType::VmStart,
            description: Some(format!("Starting VM {}", vm.name)),
            resource_id: Some(vm_id),
            resource_type: Some("vm".to_string()),
        },
    )
    .await?;
    let job_id = job.id;

    let db_pool = env.pool_arc();

    tokio::spawn(async move {
        tracing::info!(vm_id = %vm_id, job_id = %job_id, "Starting async VM start");
        #[cfg(feature = "otel")]
        let start_time = Instant::now();
        #[cfg(feature = "otel")]
        let record_vm_start_metric = |result: &str| {
            let duration = start_time.elapsed().as_secs_f64();
            let attrs = [
                KeyValue::new("result", result.to_string()),
                KeyValue::new("initial_status", initial_status_label.clone()),
            ];
            metrics
                .vm_start_job_duration_seconds
                .record(duration, &attrs);
            metrics.vm_start_jobs_total.add(1, &attrs);
        };

        if let Err(e) = jobs::mark_running(&db_pool, job_id).await {
            tracing::error!(job_id = %job_id, error = %e, "Failed to mark job as running");
            return;
        }

        let node_client = NodeClient::new(&host.address, host.port as u16);

        match ensure_vm_start_allowed(&db_pool, vm_id).await {
            Ok(()) => {}
            Err(msg) => {
                let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
                #[cfg(feature = "otel")]
                record_vm_start_metric("failed");
                return;
            }
        }

        // For a VM in Created state, call create_vm first
        if let Some(req) = create_req {
            if let Err(e) = node_client.create_vm(req).await {
                let msg = format!("create_vm failed: {:#}", e);
                tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
                let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
                let _ = vms::update_status(&db_pool, vm_id, original_status).await;
                #[cfg(feature = "otel")]
                record_vm_start_metric("failed");
                return;
            }

            if let Err(msg) = ensure_vm_start_allowed(&db_pool, vm_id).await {
                let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
                let _ = node_client.delete_vm(vm_id).await;
                let _ = vms::update_status(&db_pool, vm_id, original_status).await;
                #[cfg(feature = "otel")]
                record_vm_start_metric("failed");
                return;
            }

            let _ = jobs::update_progress(&db_pool, job_id, 50).await;
        }

        if let Err(msg) = ensure_vm_start_allowed(&db_pool, vm_id).await {
            let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
            let _ = vms::update_status(&db_pool, vm_id, original_status).await;
            #[cfg(feature = "otel")]
            record_vm_start_metric("failed");
            return;
        }

        if let Err(e) = node_client.start_vm(vm_id).await {
            let msg = format!("start_vm failed: {:#}", e);
            tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
            let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
            let _ = vms::update_status(&db_pool, vm_id, original_status).await;
            #[cfg(feature = "otel")]
            record_vm_start_metric("failed");
            return;
        }

        let _ = vms::update_status(&db_pool, vm_id, VmStatus::Running).await;
        let _ = jobs::mark_completed(&db_pool, job_id, None).await;
        #[cfg(feature = "otel")]
        record_vm_start_metric("success");

        tracing::info!(vm_id = %vm_id, job_id = %job_id, "VM start job completed");
    });

    Ok(job_id)
}

async fn ensure_vm_start_allowed(
    pool: &sqlx::PgPool,
    vm_id: Uuid,
) -> std::result::Result<(), String> {
    match vms::get(pool, vm_id).await {
        Ok(_) => {}
        Err(sqlx::Error::RowNotFound) => return Err("VM was deleted before start".to_string()),
        Err(e) => return Err(format!("failed to reload VM before start: {e}")),
    }

    match sandboxes::get_by_vm(pool, vm_id).await {
        Ok(Some(sandbox)) if sandbox.status == SandboxStatus::Destroying => {
            Err(format!("sandbox {} is being deleted", sandbox.id))
        }
        Ok(_) => Ok(()),
        Err(e) => Err(format!("failed to reload sandbox before start: {e}")),
    }
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/stop",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM stopped successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn stop(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    let host = host_for_vm(&env, vm_id).await?;
    let node_client = NodeClient::new(&host.address, host.port as u16);
    match node_client.stop_vm(vm_id).await {
        Ok(()) => {}
        Err(e)
            if e.downcast_ref::<crate::errors::Error>()
                .map(|e| matches!(e, crate::errors::Error::NotFound))
                .unwrap_or(false) =>
        {
            // VM process already gone on the node — treat as already stopped
            tracing::warn!(vm_id = %vm_id, "VM not found on node during stop, treating as already stopped");
        }
        Err(e) => {
            tracing::error!("Failed to stop VM on qarax-node: {}", e);
            return Err(crate::errors::Error::InternalServerError);
        }
    }

    vms::update_status(env.pool(), vm_id, VmStatus::Shutdown).await?;

    // Release any allocated GPUs
    if let Err(e) = host_gpus::deallocate_by_vm(env.pool(), vm_id).await {
        warn!("Failed to deallocate GPUs for VM {}: {}", vm_id, e);
    }

    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/force-stop",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM force stopped successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn force_stop(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    let host = host_for_vm(&env, vm_id).await?;
    let node_client = NodeClient::new(&host.address, host.port as u16);
    match node_client.force_stop_vm(vm_id).await {
        Ok(()) => {}
        Err(e)
            if e.downcast_ref::<crate::errors::Error>()
                .map(|e| matches!(e, crate::errors::Error::NotFound))
                .unwrap_or(false) =>
        {
            tracing::warn!(vm_id = %vm_id, "VM not found on node during force stop, treating as already stopped");
        }
        Err(e) => {
            tracing::error!("Failed to force stop VM on qarax-node: {}", e);
            return Err(crate::errors::Error::InternalServerError);
        }
    }

    vms::update_status(env.pool(), vm_id, VmStatus::Shutdown).await?;

    // Release any allocated GPUs
    if let Err(e) = host_gpus::deallocate_by_vm(env.pool(), vm_id).await {
        warn!("Failed to deallocate GPUs for VM {}: {}", vm_id, e);
    }

    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/pause",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM paused successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn pause(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    let host = host_for_vm(&env, vm_id).await?;
    let node_client = NodeClient::new(&host.address, host.port as u16);
    node_client.pause_vm(vm_id).await.map_err(|e| {
        tracing::error!("Failed to pause VM on qarax-node: {}", e);
        crate::errors::Error::InternalServerError
    })?;

    vms::update_status(env.pool(), vm_id, VmStatus::Paused).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/resume",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM resumed successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn resume(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    let host = host_for_vm(&env, vm_id).await?;
    let node_client = NodeClient::new(&host.address, host.port as u16);
    node_client.resume_vm(vm_id).await.map_err(|e| {
        tracing::error!("Failed to resume VM on qarax-node: {}", e);
        crate::errors::Error::InternalServerError
    })?;

    vms::update_status(env.pool(), vm_id, VmStatus::Running).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/vms/{vm_id}/snapshots",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier"),
        crate::handlers::NameQuery
    ),
    responses(
        (status = 200, description = "List snapshots for a VM", body = Vec<Snapshot>),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn list_snapshots(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::NameQuery>,
) -> Result<ApiResponse<Vec<Snapshot>>> {
    // Return 404 if VM doesn't exist
    let _vm = vms::get(env.pool(), vm_id).await?;

    let list = snapshots::list_for_vm(env.pool(), vm_id, query.name.as_deref()).await?;

    Ok(ApiResponse {
        data: list,
        code: StatusCode::OK,
    })
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSnapshotRequest {
    /// Human-readable name for the snapshot (auto-generated if omitted).
    pub name: Option<String>,
    /// Storage pool to place the snapshot in. Defaults to the pool of the
    /// VM's primary disk, or any active non-OverlayBD pool.
    pub storage_pool_id: Option<Uuid>,
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/snapshots",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    request_body = CreateSnapshotRequest,
    responses(
        (status = 201, description = "Snapshot created", body = Snapshot),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn create_snapshot(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<CreateSnapshotRequest>,
) -> Result<ApiResponse<Snapshot>> {
    let host = host_for_vm(&env, vm_id).await?;

    // Resolve the storage pool: explicit → VM's primary disk's pool → any active non-OverlayBD.
    let preferred_pool_id = if body.storage_pool_id.is_some() {
        body.storage_pool_id
    } else {
        let disks = vm_disks::list_by_vm(env.pool(), vm_id).await?;
        let mut found = None;
        for disk in &disks {
            if let Some(so_id) = disk.storage_object_id
                && let Ok(so) = storage_objects::get(env.pool(), so_id).await
            {
                let pool_row = storage_pools::get(env.pool(), so.storage_pool_id).await;
                if let Ok(sp) = pool_row
                    && sp.pool_type != storage_pools::StoragePoolType::OverlayBd
                {
                    found = Some(sp.id);
                    break;
                }
            }
        }
        found
    };

    let pool_id = storage_pools::pick_active_non_overlaybd(env.pool(), preferred_pool_id)
        .await?
        .ok_or_else(|| {
            crate::errors::Error::UnprocessableEntity(
                "no suitable storage pool available for snapshot".into(),
            )
        })?;

    let name = body
        .name
        .clone()
        .unwrap_or_else(|| format!("snapshot-{}", &Uuid::new_v4().to_string()[..8]));

    // Create the storage object — path is derived from the pool config.
    let so_id = storage_objects::create(
        env.pool(),
        NewStorageObject {
            name: name.clone(),
            storage_pool_id: Some(pool_id),
            object_type: StorageObjectType::Snapshot,
            size_bytes: 0,
            config: serde_json::Value::Null,
            parent_id: None,
        },
    )
    .await?;

    let so = storage_objects::get(env.pool(), so_id).await?;

    let dir_path = storage_objects::get_path_from_config(&so.config)
        .ok_or(crate::errors::Error::InternalServerError)?;
    let snapshot_url = format!("file://{}", dir_path);

    let id = snapshots::create(
        env.pool(),
        &NewSnapshot {
            vm_id,
            storage_object_id: so_id,
            name,
        },
    )
    .await?;

    let node_client = NodeClient::new(&host.address, host.port as u16);

    // Pause the VM before snapshotting
    if let Err(e) = node_client.pause_vm(vm_id).await {
        error!("Failed to pause VM before snapshot: {}", e);
        let _ = snapshots::update_status(env.pool(), id, SnapshotStatus::Failed).await;
        return Err(crate::errors::Error::InternalServerError);
    }

    // Take the snapshot
    let snap_result = node_client.snapshot_vm(vm_id, &snapshot_url).await;

    // Always attempt to resume, but capture the result — a failed resume means
    // the VM is stuck Paused and the client must be informed even if the
    // snapshot data itself is valid.
    let resume_result = node_client.resume_vm(vm_id).await;
    if let Err(e) = &resume_result {
        error!("Failed to resume VM after snapshot: {}", e);
    }

    match snap_result {
        Ok(()) => {
            snapshots::update_status(env.pool(), id, SnapshotStatus::Ready).await?;

            // Snapshot succeeded but VM is stuck Paused — return an error so
            // the client knows manual intervention is needed.
            if resume_result.is_err() {
                return Err(crate::errors::Error::InternalServerError);
            }
        }
        Err(e) => {
            error!("Failed to snapshot VM: {}", e);
            let _ = snapshots::update_status(env.pool(), id, SnapshotStatus::Failed).await;
            return Err(crate::errors::Error::InternalServerError);
        }
    }

    let snapshot = snapshots::get(env.pool(), id).await?;

    Ok(ApiResponse {
        data: snapshot,
        code: StatusCode::CREATED,
    })
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreRequest {
    pub snapshot_id: Uuid,
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/restore",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    request_body = RestoreRequest,
    responses(
        (status = 200, description = "VM restored from snapshot", body = Vm),
        (status = 404, description = "VM or snapshot not found"),
        (status = 422, description = "VM not in a restoreable state or snapshot not ready"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn restore(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<RestoreRequest>,
) -> Result<ApiResponse<Vm>> {
    let vm = vms::get(env.pool(), vm_id).await?;

    match vm.status {
        VmStatus::Running | VmStatus::Paused | VmStatus::Pending => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "VM must be stopped before restoring".into(),
            ));
        }
        VmStatus::Shutdown | VmStatus::Created | VmStatus::Unknown | VmStatus::Migrating => {}
    }

    let snapshot = snapshots::get(env.pool(), body.snapshot_id)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => crate::errors::Error::NotFound,
            _ => crate::errors::Error::Sqlx(e),
        })?;

    if snapshot.vm_id != vm_id {
        return Err(crate::errors::Error::NotFound);
    }

    if snapshot.status != SnapshotStatus::Ready {
        return Err(crate::errors::Error::UnprocessableEntity(
            "snapshot is not in ready state".into(),
        ));
    }

    let host = host_for_vm(&env, vm_id).await?;
    let node_client = NodeClient::new(&host.address, host.port as u16);

    vms::update_status(env.pool(), vm_id, VmStatus::Pending).await?;

    let so = storage_objects::get(env.pool(), snapshot.storage_object_id).await?;
    let dir_path = storage_objects::get_path_from_config(&so.config)
        .ok_or(crate::errors::Error::InternalServerError)?;
    let snapshot_url = format!("file://{}", dir_path);

    // The node handles the full restore flow: kills any existing CH process,
    // spawns a fresh one, and calls vm.restore directly (no vm.create needed).
    if let Err(e) = node_client.restore_vm(vm_id, &snapshot_url).await {
        let msg = format!("restore_vm failed: {:#}", e);
        tracing::error!(vm_id = %vm_id, error = %msg);
        let _ = vms::update_status(env.pool(), vm_id, VmStatus::Unknown).await;
        return Err(crate::errors::Error::InternalServerError);
    }

    vms::update_status(env.pool(), vm_id, VmStatus::Running).await?;

    let updated_vm = vms::get(env.pool(), vm_id).await?;
    Ok(ApiResponse {
        data: updated_vm,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/vms/{vm_id}/metrics",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "VM live metrics and counters", body = VmMetrics),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn metrics(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<VmMetrics>> {
    // Verify VM exists in DB and resolve its host
    let vm = vms::get(env.pool(), vm_id).await?;

    let host = host_for_vm(&env, vm_id).await?;
    let node_client = NodeClient::new(&host.address, host.port as u16);

    // Get live status and memory info from node
    let (status, memory_actual_size) = match node_client.get_vm_info(vm_id).await {
        Ok(state) => {
            let live_status = match state.status {
                1 => VmStatus::Created,
                2 => VmStatus::Running,
                3 => VmStatus::Paused,
                4 => VmStatus::Shutdown,
                _ => VmStatus::Unknown,
            };
            (live_status, state.memory_actual_size)
        }
        Err(_) => {
            // Node unreachable or VM not found on node — return DB status
            (vm.status, None)
        }
    };

    // Get live counters from node
    let counters = match node_client.get_vm_counters(vm_id).await {
        Ok(c) => c
            .counters
            .into_iter()
            .map(|(device, device_counters)| (device, device_counters.values))
            .collect(),
        Err(_) => HashMap::new(),
    };

    Ok(ApiResponse {
        data: VmMetrics {
            vm_id,
            status,
            memory_actual_size,
            counters,
        },
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    delete,
    path = "/vms/{vm_id}",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 204, description = "VM deleted successfully"),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    let vm = vms::get(env.pool(), vm_id).await?;

    // Release any allocated GPUs before deleting
    if let Err(e) = host_gpus::deallocate_by_vm(env.pool(), vm_id).await {
        warn!("Failed to deallocate GPUs for VM {}: {}", vm_id, e);
    }

    // Only call delete_vm on the node if the VM was ever provisioned there
    if vm.status != VmStatus::Created
        && vm.status != VmStatus::Pending
        && let Ok(host) = host_for_vm(&env, vm_id).await
    {
        let node_client = NodeClient::new(&host.address, host.port as u16);
        // Tolerate node errors on delete (VM may already be gone)
        if let Err(e) = node_client.delete_vm(vm_id).await {
            tracing::warn!("delete_vm on node failed (ignoring): {}", e);
        }
    }

    // Fire lifecycle hooks before deleting the row
    let prev_status_str = vm.status.to_string();
    if let Err(e) =
        lifecycle_hooks::enqueue_matching(env.pool(), &vm, &prev_status_str, "deleted").await
    {
        tracing::warn!("failed to enqueue delete hooks for VM {}: {}", vm_id, e);
    }

    vms::delete(env.pool(), vm_id).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::NO_CONTENT,
    })
}

#[utoipa::path(
    get,
    path = "/vms/{vm_id}/console",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 200, description = "Console log content", body = String, content_type = "text/plain"),
        (status = 404, description = "Console logging not available for this VM"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn console_log(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
) -> Result<axum::response::Response> {
    let host = host_for_vm(&env, vm_id).await?;
    let node_client = NodeClient::new(&host.address, host.port as u16);

    let response = node_client.read_console_log(vm_id).await.map_err(|e| {
        tracing::error!("Failed to read console log: {}", e);
        crate::errors::Error::InternalServerError
    })?;

    if !response.available {
        // VMs that boot directly from a block device (overlaybd) use a PTY for their
        // serial console; there is no persistent log file.  Tell the user to use
        // `vm attach` for interactive access instead.
        return Ok(axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(axum::body::Body::from(
                "(No console log file available — this VM uses a PTY serial console.\n\
                 Use `vm attach` for interactive access.)\n",
            ))
            .unwrap());
    }

    Ok(axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(axum::body::Body::from(response.content))
        .unwrap())
}

/// WebSocket console attachment for interactive terminal access
#[utoipa::path(
    get,
    path = "/vms/{vm_id}/console/attach",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    responses(
        (status = 101, description = "Switching Protocols - WebSocket connection established"),
        (status = 404, description = "VM not found"),
        (status = 422, description = "VM console not available or not in PTY mode"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env, ws))]
pub async fn console_attach(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response> {
    info!("WebSocket console attachment requested for VM: {}", vm_id);

    // Verify VM exists and get host
    let host = host_for_vm(&env, vm_id).await?;

    Ok(ws.on_upgrade(move |socket| handle_console_websocket(socket, vm_id, host)))
}

async fn handle_console_websocket(ws: WebSocket, vm_id: Uuid, host: crate::model::hosts::Host) {
    info!("WebSocket connection established for VM: {}", vm_id);

    let (mut ws_sender, mut ws_receiver) = ws.split();

    // Connect to qarax-node gRPC console stream
    let node_client = NodeClient::new(&host.address, host.port as u16);

    match node_client.attach_console(vm_id).await {
        Ok((grpc_input_tx, mut grpc_output_rx)) => {
            // Spawn task to forward WebSocket -> gRPC
            let ws_to_grpc = tokio::spawn(async move {
                while let Some(msg) = ws_receiver.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if grpc_input_tx.send(text.as_bytes().to_vec()).await.is_err() {
                                break;
                            }
                        }
                        Ok(Message::Binary(data)) => {
                            if grpc_input_tx.send(data.to_vec()).await.is_err() {
                                break;
                            }
                        }
                        Ok(Message::Close(_)) => {
                            info!("WebSocket client closed connection for VM {}", vm_id);
                            break;
                        }
                        Err(e) => {
                            warn!("WebSocket error for VM {}: {}", vm_id, e);
                            break;
                        }
                        _ => {}
                    }
                }
            });

            // Spawn task to forward gRPC -> WebSocket
            let grpc_to_ws = tokio::spawn(async move {
                while let Some(result) = grpc_output_rx.recv().await {
                    match result {
                        Ok(data) => {
                            if ws_sender.send(Message::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(error_msg) => {
                            error!("gRPC console error for VM {}: {}", vm_id, error_msg);
                            let _ = ws_sender
                                .send(Message::Text(format!("Error: {}", error_msg).into()))
                                .await;
                            break;
                        }
                    }
                }
            });

            // Wait for either task to complete
            tokio::select! {
                _ = ws_to_grpc => {
                    info!("WebSocket to gRPC task completed for VM {}", vm_id);
                }
                _ = grpc_to_ws => {
                    info!("gRPC to WebSocket task completed for VM {}", vm_id);
                }
            }
        }
        Err(e) => {
            error!("Failed to attach to console for VM {}: {:#}", vm_id, e);
            let _ = ws_sender
                .send(Message::Text(
                    format!("Failed to attach to console: {:#}", e).into(),
                ))
                .await;
        }
    }

    info!("WebSocket console session ended for VM: {}", vm_id);
}

fn subnet_mask_from_cidr(subnet: &str) -> Option<String> {
    let prefix = subnet
        .split_once('/')
        .and_then(|(_, p)| p.parse::<u32>().ok())?;
    if prefix > 32 {
        return None;
    }
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    Some(format!(
        "{}.{}.{}.{}",
        (mask >> 24) & 0xFF,
        (mask >> 16) & 0xFF,
        (mask >> 8) & 0xFF,
        mask & 0xFF
    ))
}

/// Build a `CreateVmRequest` from DB state — called at `vm start` for lazily-provisioned VMs.
async fn build_create_vm_request(env: &App, vm: &Vm) -> Result<CreateVmRequest> {
    let vm_id = vm.id;

    // Resolve boot payload based on boot_mode
    let (kernel, firmware, initramfs, default_cmdline) = match vm.boot_mode {
        BootMode::Firmware => {
            let d = env.vm_defaults();
            let fw = d.firmware.as_ref().filter(|s| !s.is_empty()).map(|s| s.to_string()).ok_or_else(|| {
                crate::errors::Error::UnprocessableEntity(
                    "firmware boot mode requires a firmware path (set vm_defaults.firmware or VM_FIRMWARE env var)".into(),
                )
            })?;
            (None, Some(fw), None, None)
        }
        BootMode::Kernel => {
            let (k, i, c) = if let Some(boot_source_id) = vm.boot_source_id {
                let resolved = boot_sources::resolve(env.pool(), boot_source_id)
                    .await
                    .map_err(|e| {
                        crate::errors::Error::UnprocessableEntity(format!(
                            "Invalid boot_source_id: {}",
                            e
                        ))
                    })?;
                (
                    resolved.kernel_path,
                    resolved.initramfs_path,
                    resolved.kernel_params,
                )
            } else {
                let d = env.vm_defaults();
                (
                    d.kernel.as_ref().to_string(),
                    d.initramfs
                        .as_ref()
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string()),
                    d.cmdline.as_ref().to_string(),
                )
            };
            (Some(k), None, i, Some(c))
        }
    };

    // Load disks, filesystems, and networks
    let db_disks = vm_disks::list_by_vm(env.pool(), vm_id).await?;
    let filesystems = vm_filesystems::list_by_vm(env.pool(), vm_id).await?;
    let db_networks = network_interfaces::list_by_vm(env.pool(), vm_id).await?;

    let mut networks = net_configs_from_db(&db_networks);

    let mut ip_params = String::new();

    // Managed networking stores IPs in DB as inet (often /32); provide mask from network CIDR.
    // For passt networks, switch to vhost-user passt backend and let guest networking be dynamic.
    for (i, db_net) in db_networks.iter().enumerate() {
        if let Some(net_id) = db_net.network_id
            && let Some(net_config) = networks.get_mut(i)
            && let Ok(network) = networks::get(env.pool(), net_id).await
        {
            if network.network_type.as_deref() == Some("passt") {
                net_config.vhost_user = Some(true);
                net_config.vhost_socket = Some("passt".to_string());
                net_config.vhost_mode = Some(VhostMode::Client as i32);
                net_config.tap = None;
                net_config.bridge = None;
                net_config.ip = None;
                net_config.mask = None;
            } else if net_config.ip.is_some() {
                if net_config.mask.is_none() {
                    net_config.mask = subnet_mask_from_cidr(&network.subnet);
                }

                let ty = network.network_type.as_deref();
                if ty == Some("bridge") || ty == Some("isolated") {
                    let ip = net_config.ip.as_ref().unwrap();
                    let mask = net_config.mask.as_deref().unwrap_or("");
                    let gw = network.gateway.as_deref().unwrap_or("");
                    let dns = network.dns.as_deref().unwrap_or("");
                    ip_params.push_str(&format!(
                        " ip={}::{}:{}::eth{}:off:{}",
                        ip, gw, mask, i, dns
                    ));
                }
            }
        }
    }

    // Populate bridge field from host_networks if the VM has a host and network interfaces
    if let Some(host_id) = vm.host_id {
        for (i, db_net) in db_networks.iter().enumerate() {
            if let Some(net_id) = db_net.network_id
                && let Ok(Some(bridge_name)) =
                    networks::get_host_bridge(env.pool(), host_id, net_id).await
                && let Some(net_config) = networks.get_mut(i)
            {
                net_config.bridge = Some(bridge_name);
            }
        }
    }

    // Batch-fetch storage objects and pools to avoid N+1 queries.
    // Include both primary (storage_object_id) and upper layer (upper_storage_object_id) SOs.
    let so_ids: Vec<Uuid> = db_disks
        .iter()
        .flat_map(|d| {
            [d.storage_object_id, d.upper_storage_object_id]
                .into_iter()
                .flatten()
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let objects = storage_objects::get_batch(env.pool(), &so_ids).await?;
    let objects_map: std::collections::HashMap<Uuid, _> =
        objects.into_iter().map(|o| (o.id, o)).collect();

    let pool_ids: Vec<Uuid> = objects_map
        .values()
        .map(|o| o.storage_pool_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let pools = storage_pools::get_batch(env.pool(), &pool_ids).await?;
    let pools_map: std::collections::HashMap<Uuid, _> =
        pools.into_iter().map(|p| (p.id, p)).collect();

    // Verify that the VM's host has access to all LOCAL storage pools
    if let Some(host_id) = vm.host_id {
        for pool in pools_map.values() {
            if pool.pool_type == storage_pools::StoragePoolType::Local {
                let has_pool = storage_pools::host_has_pool(env.pool(), host_id, pool.id).await?;
                if !has_pool {
                    return Err(crate::errors::Error::UnprocessableEntity(format!(
                        "Host {} is not attached to local storage pool {}",
                        host_id, pool.id
                    )));
                }
            }
        }
    }

    // Resolve each vm_disk to a DiskConfig using the pre-fetched maps
    let mut resolved_disks: Vec<DiskConfig> = Vec::new();
    let mut has_overlaybd_boot = false;

    for disk in &db_disks {
        if let Some(so_id) = disk.storage_object_id {
            let obj = objects_map
                .get(&so_id)
                .ok_or(crate::errors::Error::NotFound)?;
            let pool = pools_map
                .get(&obj.storage_pool_id)
                .ok_or(crate::errors::Error::NotFound)?;

            match pool.pool_type {
                storage_pools::StoragePoolType::OverlayBd => {
                    let image_ref = obj
                        .config
                        .get("image_ref")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let registry_url = obj
                        .config
                        .get("registry_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();

                    let upper_object = disk
                        .upper_storage_object_id
                        .and_then(|uid| objects_map.get(&uid));
                    let (upper_data_path, upper_index_path) = overlaybd_upper_paths(upper_object);

                    resolved_disks.push(disk_to_disk_config(
                        disk,
                        None,
                        Some(image_ref),
                        Some(registry_url),
                        upper_data_path,
                        upper_index_path,
                    ));
                    has_overlaybd_boot = true;
                }
                storage_pools::StoragePoolType::Local | storage_pools::StoragePoolType::Nfs => {
                    let path = storage_objects::get_path_from_config(&obj.config);
                    resolved_disks.push(disk_to_disk_config(disk, path, None, None, None, None));
                }
            }
        }
    }

    // Suppress kernel ip= params if cloud-init will configure networking instead.
    if vm.cloud_init_network_config.is_some() {
        ip_params.clear();
    }

    let has_vhost_user_network = networks.iter().any(|n| n.vhost_user.unwrap_or(false));

    // Choose cmdline based on what's attached
    let (fs_configs, cmdline, memory_shared) = if has_overlaybd_boot {
        (
            vec![],
            // net.ifnames=0: keep eth0 naming (prevents udev renaming to ens*/enp*)
            // init=/.qarax-init: use our injected init binary instead of the OCI image's /sbin/init
            Some(format!(
                "console=ttyS0 root=/dev/vda rw net.ifnames=0 biosdevname=0 init=/.qarax-init{}",
                ip_params
            )),
            false,
        )
    } else if !filesystems.is_empty() {
        let fs = &filesystems[0];
        let socket_path = format!("/var/lib/qarax/vms/{}-fs0.sock", vm_id);
        // Derive the rootfs path from image_ref so the node can start virtiofsd.
        // The image store uses: <cache_dir>/<safe_name>/rootfs
        // where safe_name replaces '/', ':', '@' with '_'.
        let bootstrap_path = fs.image_ref.as_deref().map(image_ref_to_rootfs_path);
        let fs_config = FsConfig {
            tag: fs.tag.clone(),
            socket: socket_path,
            num_queues: fs.num_queues,
            queue_size: fs.queue_size,
            pci_segment: fs.pci_segment,
            id: Some("fs0".to_string()),
            bootstrap_path,
        };
        (
            vec![fs_config],
            Some(format!(
                "console=ttyS0 root=rootfs rootfstype=virtiofs rw init=/.qarax-init{}",
                ip_params
            )),
            true,
        )
    } else {
        // Plain boot with kernel/initramfs only (or firmware — cmdline stays None)
        let c = default_cmdline.map(|s| format!("{}{}", s, ip_params));
        (vec![], c, vm.memory_shared)
    };

    // For overlaybd boot, don't pass an initramfs — the kernel mounts /dev/vda (ext4)
    // directly. The test initramfs contains /.qarax-config.json which prevents switch_root.
    let initramfs = if has_overlaybd_boot { None } else { initramfs };

    // Auto-generate meta-data if not provided by the caller.
    let cloud_init_meta_data = vm.cloud_init_meta_data.clone().or_else(|| {
        vm.cloud_init_user_data.as_ref()?;
        Some(format!(
            "instance-id: {}\nlocal-hostname: {}\n",
            vm.id, vm.name
        ))
    });

    // Build VFIO device configs from allocated GPUs
    let allocated_gpus = host_gpus::list_by_vm(env.pool(), vm_id).await?;
    let devices: Vec<_> = allocated_gpus
        .iter()
        .enumerate()
        .map(|(i, gpu)| crate::grpc_client::node::VfioDeviceConfig {
            id: format!("gpu{}", i),
            path: format!("/sys/bus/pci/devices/{}", gpu.pci_address),
            iommu: Some(true),
            pci_segment: None,
        })
        .collect();

    // Compute NUMA placement if applicable
    let numa_placement = compute_numa_placement(env, vm, &allocated_gpus).await;

    Ok(CreateVmRequest {
        vm_id,
        boot_vcpus: vm.boot_vcpus,
        max_vcpus: vm.max_vcpus,
        memory_size: vm.memory_size,
        networks,
        kernel,
        firmware,
        initramfs,
        cmdline,
        fs_configs,
        memory_shared: memory_shared || has_vhost_user_network,
        memory_hotplug_size: vm.memory_hotplug_size,
        memory_hugepages: vm.memory_hugepages,
        disks: resolved_disks,
        cloud_init_user_data: vm.cloud_init_user_data.clone(),
        cloud_init_meta_data,
        cloud_init_network_config: vm.cloud_init_network_config.clone(),
        devices,
        vsock: vm
            .config
            .get("sandbox_exec")
            .and_then(|value| value.as_bool())
            .filter(|enabled| *enabled)
            .map(|_| VsockConfig {
                cid: None,
                socket: None,
                iommu: None,
                pci_segment: None,
                id: Some("sandbox-exec".to_string()),
            }),
        numa_placement,
    })
}

/// Compute NUMA placement for a VM at start time.
///
/// Priority:
/// 1. GPU-local NUMA: if the VM has allocated GPUs with known NUMA nodes, pin to those nodes.
/// 2. Explicit NUMA: if `numa_config.numa_node` is set in `vm.config`, pin to that node.
/// 3. None: no NUMA pinning.
async fn compute_numa_placement(
    env: &App,
    vm: &Vm,
    allocated_gpus: &[host_gpus::HostGpu],
) -> Option<NumaPlacement> {
    let host_id = vm.host_id?;

    // Determine which NUMA node(s) to target
    let target_node_ids: Vec<i32> = if !allocated_gpus.is_empty() {
        // Collect distinct NUMA nodes from the allocated GPUs (excluding -1 = unknown)
        let mut node_ids: Vec<i32> = allocated_gpus
            .iter()
            .map(|g| g.numa_node)
            .filter(|&n| n >= 0)
            .collect();
        node_ids.sort_unstable();
        node_ids.dedup();
        node_ids
    } else {
        // Check for explicit numa_config stored in vm.config at creation time
        let numa_cfg = vm
            .config
            .get("numa_config")
            .and_then(host_gpus::NumaConfig::from_value);
        if let Some(cfg) = numa_cfg {
            cfg.numa_node.map(|n| vec![n]).unwrap_or_default()
        } else {
            vec![]
        }
    };

    if target_node_ids.is_empty() {
        return None;
    }

    // Load the NUMA nodes for this host from DB
    let host_nodes = match host_numa::list_by_host(env.pool(), host_id).await {
        Ok(nodes) => nodes,
        Err(e) => {
            warn!(
                vm_id = %vm.id,
                "Failed to load NUMA topology for host {}: {}",
                host_id, e
            );
            return None;
        }
    };

    if host_nodes.is_empty() {
        return None;
    }

    // Collect all host CPUs belonging to the target NUMA nodes
    let host_cpus: Vec<i32> = host_nodes
        .iter()
        .filter(|n| target_node_ids.contains(&n.node_id))
        .flat_map(|n| expand_cpu_list(&n.cpu_list))
        .collect();

    if host_cpus.is_empty() {
        return None;
    }

    // Build per-vCPU pinning: all vCPUs share the same host CPU set
    let cpu_pinning: Vec<CpuPinning> = (0..vm.boot_vcpus)
        .map(|vcpu| CpuPinning {
            vcpu,
            host_cpus: host_cpus.clone(),
        })
        .collect();

    // Memory zone IDs — one per NUMA node
    let memory_zone_ids: Vec<String> = target_node_ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("zone{}", i))
        .collect();

    Some(NumaPlacement {
        host_numa_node_ids: target_node_ids,
        cpu_pinning,
        memory_zone_ids,
    })
}

/// Pick a unique disk ID for a new disk given the existing disks on a VM.
/// Uses the format `disk{N}` where N is the next available index.
fn next_disk_id(existing: &[vm_disks::VmDisk]) -> String {
    let used: std::collections::HashSet<&str> =
        existing.iter().map(|d| d.logical_name.as_str()).collect();
    for i in 0u32.. {
        let id = format!("disk{}", i);
        if !used.contains(id.as_str()) {
            return id;
        }
    }
    unreachable!()
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AttachDiskRequest {
    /// Storage object ID (must be `oci_image` type).
    pub storage_object_id: Uuid,
    /// Logical device name inside the VM (e.g. "vda"); auto-generated if omitted.
    pub logical_name: Option<String>,
    /// Boot priority — lower is higher priority (default: `0`).
    pub boot_order: Option<i32>,
}

/// Attach a storage object to a VM as a disk.
/// For VMs in `Created` state the disk is only recorded in the database and will be
/// passed to Cloud Hypervisor when the VM starts.  For VMs in `Running` or `Shutdown`
/// state the disk is recorded **and** immediately applied via the Cloud Hypervisor API
/// (CH keeps the VM definition after shutdown, so disk changes are accepted).
#[utoipa::path(
    post,
    path = "/vms/{vm_id}/disks",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    request_body = AttachDiskRequest,
    responses(
        (status = 201, description = "Disk attached (and applied to CH if VM is running or shutdown)", body = crate::model::vm_disks::VmDisk),
        (status = 404, description = "VM or storage object not found"),
        (status = 422, description = "VM not in Created, Running, or Shutdown state"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn attach_disk(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
    Json(req): Json<AttachDiskRequest>,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse as _;

    let vm = vms::get(env.pool(), vm_id).await?;
    match vm.status {
        VmStatus::Created | VmStatus::Running | VmStatus::Shutdown => {}
        _ => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "Disks can only be attached to VMs in Created, Running, or Shutdown state".into(),
            ));
        }
    }

    // Verify the storage object exists and check local pool affinity
    let obj = storage_objects::get(env.pool(), req.storage_object_id).await?;
    let pool = storage_pools::get(env.pool(), obj.storage_pool_id).await?;
    if pool.pool_type == storage_pools::StoragePoolType::Local {
        let host_id = vm.host_id.ok_or_else(|| {
            crate::errors::Error::UnprocessableEntity(
                "Cannot attach a local disk to a VM with no assigned host".into(),
            )
        })?;
        let has_pool = storage_pools::host_has_pool(env.pool(), host_id, pool.id).await?;
        if !has_pool {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "VM's host {} is not attached to local storage pool {}",
                host_id, pool.id
            )));
        }
    }

    let existing = vm_disks::list_by_vm(env.pool(), vm_id).await?;
    if existing
        .iter()
        .any(|d| d.storage_object_id == Some(req.storage_object_id))
    {
        return Err(crate::errors::Error::Conflict(format!(
            "Storage object {} is already attached to this VM",
            req.storage_object_id
        )));
    }

    let logical_name = match req.logical_name.clone() {
        Some(name) => name,
        None => next_disk_id(&existing),
    };
    let boot_order = req.boot_order.unwrap_or_else(|| {
        existing
            .iter()
            .filter_map(|d| d.boot_order)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0)
    });
    let disk_record = NewVmDisk {
        vm_id,
        storage_object_id: Some(req.storage_object_id),
        logical_name: logical_name.clone(),
        device_path: format!("/dev/{}", logical_name),
        boot_order: Some(boot_order),
        read_only: Some(false),
        ..Default::default()
    };

    let id = vm_disks::create(env.pool(), &disk_record).await?;
    let disk = vm_disks::get(env.pool(), id).await?;

    // If the VM is running or shutdown, apply the disk via gRPC immediately.
    // Cloud Hypervisor keeps the VM definition after shutdown (CH "Created" state),
    // so add-disk works on both running and stopped VMs.
    if matches!(vm.status, VmStatus::Running | VmStatus::Shutdown) {
        let host = host_for_vm(&env, vm_id).await?;
        let upper_object = match disk.upper_storage_object_id {
            Some(object_id) => Some(storage_objects::get(env.pool(), object_id).await?),
            None => None,
        };
        let disk_config = disk_config_for_hotplug(&disk, &obj, &pool, upper_object.as_ref());
        if let Err(e) = NodeClient::new(&host.address, host.port as u16)
            .add_disk_device(vm_id, disk_config)
            .await
        {
            error!("Failed to add disk to VM {}: {}", vm_id, e);
            if let Err(delete_err) = vm_disks::delete(env.pool(), disk.id).await {
                error!(
                    "Failed to clean up vm_disk record {} after add-disk failure: {}",
                    disk.id, delete_err
                );
            }
            return Err(crate::errors::Error::InternalServerError);
        }
    }

    Ok(ApiResponse {
        data: disk,
        code: StatusCode::CREATED,
    }
    .into_response())
}

/// Build a `DiskConfig` proto from a `VmDisk` + resolved storage object/pool (no DB calls).
fn disk_config_for_hotplug(
    disk: &vm_disks::VmDisk,
    obj: &storage_objects::StorageObject,
    pool: &storage_pools::StoragePool,
    upper_object: Option<&storage_objects::StorageObject>,
) -> DiskConfig {
    match pool.pool_type {
        storage_pools::StoragePoolType::OverlayBd => {
            let image_ref = obj
                .config
                .get("image_ref")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let registry_url = obj
                .config
                .get("registry_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let (upper_data_path, upper_index_path) = overlaybd_upper_paths(upper_object);
            disk_to_disk_config(
                disk,
                None,
                Some(image_ref),
                Some(registry_url),
                upper_data_path,
                upper_index_path,
            )
        }
        storage_pools::StoragePoolType::Local | storage_pools::StoragePoolType::Nfs => {
            let path = storage_objects::get_path_from_config(&obj.config);
            disk_to_disk_config(disk, path, None, None, None, None)
        }
    }
}

fn overlaybd_upper_paths(
    upper_object: Option<&storage_objects::StorageObject>,
) -> (Option<String>, Option<String>) {
    let upper_data_path = upper_object
        .and_then(|obj| obj.config.get("upper_data"))
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let upper_index_path = upper_object
        .and_then(|obj| obj.config.get("upper_index"))
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    (upper_data_path, upper_index_path)
}

#[derive(Debug)]
struct DiskResizeTarget {
    storage_object_id: Uuid,
    path: String,
}

fn resolve_disk_resize_target(
    disk: &vm_disks::VmDisk,
    object: &storage_objects::StorageObject,
    pool: &storage_pools::StoragePool,
) -> Result<DiskResizeTarget> {
    match (&pool.pool_type, &object.object_type) {
        (
            storage_pools::StoragePoolType::Local | storage_pools::StoragePoolType::Nfs,
            StorageObjectType::Disk | StorageObjectType::Snapshot,
        ) => {
            let path = storage_objects::get_path_from_config(&object.config).ok_or_else(|| {
                crate::errors::Error::UnprocessableEntity("disk has no resolvable host path".into())
            })?;
            Ok(DiskResizeTarget {
                storage_object_id: object.id,
                path,
            })
        }
        (storage_pools::StoragePoolType::OverlayBd, StorageObjectType::OciImage) => {
            let reason = if disk.upper_storage_object_id.is_some() {
                "persistent OverlayBD disk resize is not supported yet"
            } else {
                "ephemeral OverlayBD disks cannot be resized"
            };
            Err(crate::errors::Error::UnprocessableEntity(reason.into()))
        }
        (storage_pools::StoragePoolType::OverlayBd, _) => {
            Err(crate::errors::Error::UnprocessableEntity(
                "disk resize is not supported for this OverlayBD storage object".into(),
            ))
        }
        (storage_pools::StoragePoolType::Local | storage_pools::StoragePoolType::Nfs, _) => {
            Err(crate::errors::Error::UnprocessableEntity(
                "disk resize is only supported for disk and snapshot storage objects".into(),
            ))
        }
    }
}

/// Build a `DiskConfig` proto from a `VmDisk` plus pre-resolved path/image info.
fn disk_to_disk_config(
    disk: &vm_disks::VmDisk,
    path: Option<String>,
    oci_image_ref: Option<String>,
    registry_url: Option<String>,
    upper_data_path: Option<String>,
    upper_index_path: Option<String>,
) -> DiskConfig {
    DiskConfig {
        id: disk.logical_name.clone(),
        path,
        readonly: Some(disk.read_only),
        direct: disk.direct.then_some(true),
        vhost_user: disk.vhost_user.then_some(true),
        vhost_socket: disk.vhost_socket.clone(),
        num_queues: Some(disk.num_queues),
        queue_size: Some(disk.queue_size),
        rate_limiter: None,
        rate_limit_group: disk.rate_limit_group.clone(),
        pci_segment: (disk.pci_segment != 0).then_some(disk.pci_segment),
        serial: disk.serial_number.clone(),
        oci_image_ref,
        registry_url,
        upper_data_path,
        upper_index_path,
    }
}

/// Build a `NetConfig` proto message from a `NetworkInterface` record, resolving
/// network-type-specific settings (passt, bridge, mask) for a specific host.
async fn net_config_for_hotplug(
    env: &App,
    nic: &NetworkInterface,
    host_id: Uuid,
) -> Result<NetConfig> {
    let mut net_config = net_configs_from_db(std::slice::from_ref(nic))
        .into_iter()
        .next()
        .expect("net_configs_from_db always returns one entry per input");

    if let Some(net_id) = nic.network_id {
        let (network_result, bridge_result) = tokio::join!(
            networks::get(env.pool(), net_id),
            networks::get_host_bridge(env.pool(), host_id, net_id),
        );
        if let Ok(network) = network_result {
            if network.network_type.as_deref() == Some("passt") {
                net_config.vhost_user = Some(true);
                net_config.vhost_socket = Some("passt".to_string());
                net_config.vhost_mode = Some(VhostMode::Client as i32);
                net_config.tap = None;
                net_config.bridge = None;
                net_config.ip = None;
                net_config.mask = None;
            } else if net_config.ip.is_some() && net_config.mask.is_none() {
                net_config.mask = subnet_mask_from_cidr(&network.subnet);
            }
        }
        if let Ok(Some(bridge)) = bridge_result {
            net_config.bridge = Some(bridge);
        }
    }

    Ok(net_config)
}

/// Auto-generate the next available NIC device ID (net0, net1, …).
fn next_net_id(existing: &[NetworkInterface]) -> String {
    let used: std::collections::HashSet<&str> =
        existing.iter().map(|n| n.device_id.as_str()).collect();
    for i in 0u32.. {
        let id = format!("net{i}");
        if !used.contains(id.as_str()) {
            return id;
        }
    }
    unreachable!()
}

#[utoipa::path(
    delete,
    path = "/vms/{vm_id}/disks/{device_id}",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier"),
        ("device_id" = String, Path, description = "Disk logical name (e.g. \"disk0\")")
    ),
    responses(
        (status = 204, description = "Disk removed (and removed from CH if VM was running or shutdown)"),
        (status = 404, description = "VM or disk not found"),
        (status = 422, description = "VM not in Created, Running, or Shutdown state"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn remove_disk(
    Extension(env): Extension<App>,
    Path((vm_id, device_id)): Path<(Uuid, String)>,
) -> Result<ApiResponse<()>> {
    let vm = vms::get(env.pool(), vm_id).await?;
    match vm.status {
        VmStatus::Created | VmStatus::Running | VmStatus::Shutdown => {}
        _ => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "Disks can only be removed from VMs in Created, Running, or Shutdown state".into(),
            ));
        }
    }

    let disk = vm_disks::get_by_logical_name(env.pool(), vm_id, &device_id)
        .await?
        .ok_or(crate::errors::Error::NotFound)?;

    // Remove from CH if the VM has been created on the node (Running or Shutdown).
    // CH keeps the VM definition after shutdown, so remove-disk works in both states.
    if matches!(vm.status, VmStatus::Running | VmStatus::Shutdown) {
        let host = host_for_vm(&env, vm_id).await?;
        NodeClient::new(&host.address, host.port as u16)
            .remove_disk_device(vm_id, &device_id)
            .await
            .map_err(|e| {
                error!(
                    "Failed to remove disk {} from VM {}: {}",
                    device_id, vm_id, e
                );
                crate::errors::Error::InternalServerError
            })?;
    }

    vm_disks::delete(env.pool(), disk.id).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::NO_CONTENT,
    })
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/nics",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    request_body = NewVmNetwork,
    responses(
        (status = 201, description = "NIC added (and hotplugged if VM is running)", body = NetworkInterface),
        (status = 404, description = "VM not found"),
        (status = 409, description = "Device ID already in use"),
        (status = 422, description = "VM not in Created or Running state"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn add_nic(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
    Json(mut req): Json<NewVmNetwork>,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse as _;

    let vm = vms::get(env.pool(), vm_id).await?;
    match vm.status {
        VmStatus::Created | VmStatus::Running => {}
        _ => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "NICs can only be added to VMs in Created or Running state".into(),
            ));
        }
    }

    let existing = network_interfaces::list_by_vm(env.pool(), vm_id).await?;

    // Auto-generate device ID if not provided or empty
    if req.id.is_empty() {
        req.id = next_net_id(&existing);
    }

    if existing.iter().any(|n| n.device_id == req.id) {
        return Err(crate::errors::Error::Conflict(format!(
            "NIC device ID '{}' is already in use on this VM",
            req.id
        )));
    }

    // IP allocation for managed networking
    let req = if let Some(network_id) = req.network_id {
        if req.ip.is_none() {
            let ip = networks::next_available_ip(env.pool(), network_id)
                .await?
                .ok_or_else(|| {
                    crate::errors::Error::UnprocessableEntity("No available IPs in network".into())
                })?;
            networks::allocate_ip(env.pool(), network_id, &ip, Some(vm_id)).await?;
            NewVmNetwork {
                ip: Some(ip),
                ..req
            }
        } else {
            networks::allocate_ip(
                env.pool(),
                network_id,
                req.ip.as_deref().unwrap(),
                Some(vm_id),
            )
            .await?;
            req
        }
    } else {
        req
    };

    let mut tx = env.pool().begin().await?;
    let nic_id = network_interfaces::create(&mut tx, vm_id, &req).await?;
    tx.commit().await?;

    let nic = network_interfaces::get(env.pool(), nic_id).await?;

    if vm.status == VmStatus::Running {
        let host = host_for_vm(&env, vm_id).await?;
        let net_config = net_config_for_hotplug(&env, &nic, host.id).await?;
        if let Err(e) = NodeClient::new(&host.address, host.port as u16)
            .add_network_device(vm_id, net_config)
            .await
        {
            error!("Failed to hotplug NIC to running VM {}: {}", vm_id, e);
            if let Err(delete_err) = network_interfaces::delete(env.pool(), nic.id).await {
                error!(
                    "Failed to clean up network_interface record {} after hotplug failure: {}",
                    nic.id, delete_err
                );
            }
            return Err(crate::errors::Error::InternalServerError);
        }
    }

    Ok(ApiResponse {
        data: nic,
        code: StatusCode::CREATED,
    }
    .into_response())
}

#[utoipa::path(
    delete,
    path = "/vms/{vm_id}/nics/{device_id}",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier"),
        ("device_id" = String, Path, description = "NIC device ID (e.g. \"net0\")")
    ),
    responses(
        (status = 204, description = "NIC removed (and hotunplugged if VM was running)"),
        (status = 404, description = "VM or NIC not found"),
        (status = 422, description = "VM not in Created or Running state"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn remove_nic(
    Extension(env): Extension<App>,
    Path((vm_id, device_id)): Path<(Uuid, String)>,
) -> Result<ApiResponse<()>> {
    let vm = vms::get(env.pool(), vm_id).await?;
    match vm.status {
        VmStatus::Created | VmStatus::Running => {}
        _ => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "NICs can only be removed from VMs in Created or Running state".into(),
            ));
        }
    }

    let nic = network_interfaces::get_by_device_id(env.pool(), vm_id, &device_id)
        .await?
        .ok_or(crate::errors::Error::NotFound)?;

    if vm.status == VmStatus::Running {
        let host = host_for_vm(&env, vm_id).await?;
        NodeClient::new(&host.address, host.port as u16)
            .remove_network_device(vm_id, &device_id)
            .await
            .map_err(|e| {
                error!(
                    "Failed to hotunplug NIC {} from VM {}: {}",
                    device_id, vm_id, e
                );
                crate::errors::Error::InternalServerError
            })?;
    }

    network_interfaces::delete(env.pool(), nic.id).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::NO_CONTENT,
    })
}

/// Request body for `POST /vms/{vm_id}/migrate`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct VmMigrateRequest {
    /// UUID of the destination host to migrate the VM to.
    pub target_host_id: Uuid,
}

/// Response body for `POST /vms/{vm_id}/migrate`.
#[derive(Serialize, ToSchema)]
pub struct VmMigrateResponse {
    pub job_id: Uuid,
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/template",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    request_body = CreateVmTemplateFromVmRequest,
    responses(
        (status = 201, description = "VM template created from VM", body = String),
        (status = 404, description = "VM not found"),
        (status = 409, description = "VM template with name already exists"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn create_template_from_vm(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
    Json(request): Json<CreateVmTemplateFromVmRequest>,
) -> Result<(StatusCode, String)> {
    let id = vm_templates::create_from_vm(env.pool(), vm_id, request).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    post,
    path = "/vms/{vm_id}/migrate",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    request_body = VmMigrateRequest,
    responses(
        (status = 202, description = "Migration accepted", body = VmMigrateResponse),
        (status = 404, description = "VM or host not found"),
        (status = 422, description = "VM not in a migratable state"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn migrate(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
    Json(req): Json<VmMigrateRequest>,
) -> Result<axum::response::Response> {
    let vm = vms::get(env.pool(), vm_id).await?;

    match vm.status {
        VmStatus::Running | VmStatus::Paused => {}
        _ => {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "VM must be running or paused to migrate (current status: {})",
                vm.status
            )));
        }
    }

    let source_host = host_for_vm(&env, vm_id).await?;
    let target_host = hosts::require_by_id(env.pool(), req.target_host_id).await?;

    if source_host.id == target_host.id {
        return Err(crate::errors::Error::UnprocessableEntity(
            "Source and destination hosts are the same".into(),
        ));
    }

    // Live migration requires all disks to be on shared storage pools.
    // NFS pools share the same filesystem path; OverlayBD pools share
    // the same OCI registry (the destination mounts a fresh TCMU device).
    // Local pools are node-local and cannot be migrated.
    let db_disks = vm_disks::list_by_vm(env.pool(), vm_id).await?;

    // Check primary storage objects.
    let so_ids: Vec<Uuid> = db_disks
        .iter()
        .filter_map(|d| d.storage_object_id)
        .collect();
    if !so_ids.is_empty() {
        let objects = storage_objects::get_batch(env.pool(), &so_ids).await?;
        let pool_ids: Vec<Uuid> = objects
            .iter()
            .map(|o| o.storage_pool_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let pools = storage_pools::get_batch(env.pool(), &pool_ids).await?;
        for pool in &pools {
            if !pool.pool_type.supports_live_migration() {
                return Err(crate::errors::Error::UnprocessableEntity(format!(
                    "live migration is not supported for pool '{}' (type: {:?})",
                    pool.name, pool.pool_type
                )));
            }
            let dest_has_pool =
                storage_pools::host_has_pool(env.pool(), target_host.id, pool.id).await?;
            if !dest_has_pool {
                return Err(crate::errors::Error::UnprocessableEntity(format!(
                    "destination host {} does not have pool '{}' ({:?}) attached",
                    target_host.id, pool.name, pool.pool_type
                )));
            }
        }
    }

    // Check upper layer storage objects (persistent OverlayBD).
    // Local upper layers cannot be live-migrated; NFS upper layers can.
    let upper_so_ids: Vec<Uuid> = db_disks
        .iter()
        .filter_map(|d| d.upper_storage_object_id)
        .collect();
    if !upper_so_ids.is_empty() {
        let upper_objects = storage_objects::get_batch(env.pool(), &upper_so_ids).await?;
        let upper_pool_ids: Vec<Uuid> = upper_objects
            .iter()
            .map(|o| o.storage_pool_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let upper_pools = storage_pools::get_batch(env.pool(), &upper_pool_ids).await?;
        for pool in &upper_pools {
            if !pool.pool_type.supports_live_migration() {
                return Err(crate::errors::Error::UnprocessableEntity(format!(
                    "live migration is not supported: persistent upper layer is on local pool '{}'",
                    pool.name
                )));
            }
            let dest_has_pool =
                storage_pools::host_has_pool(env.pool(), target_host.id, pool.id).await?;
            if !dest_has_pool {
                return Err(crate::errors::Error::UnprocessableEntity(format!(
                    "destination host {} does not have upper layer pool '{}' attached",
                    target_host.id, pool.name
                )));
            }
        }
    }

    // Build the VmConfig that the destination node needs to set up infrastructure.
    let create_req = build_create_vm_request(&env, &vm).await?;

    let original_status = vm.status.clone();

    vms::update_status(env.pool(), vm_id, VmStatus::Migrating).await?;

    let job = jobs::create(
        env.pool(),
        NewJob {
            job_type: JobType::VmMigrate,
            description: Some(format!(
                "Migrating VM {} from host {} to {}",
                vm.name, source_host.id, target_host.id
            )),
            resource_id: Some(vm_id),
            resource_type: Some("vm".to_string()),
        },
    )
    .await?;
    let job_id = job.id;
    let db_pool = env.pool_arc();

    tokio::spawn(async move {
        tracing::info!(vm_id = %vm_id, job_id = %job_id, "Starting async VM migration");

        if let Err(e) = jobs::mark_running(&db_pool, job_id).await {
            tracing::error!(job_id = %job_id, error = %e, "Failed to mark job as running");
            return;
        }

        let source_client = NodeClient::new(&source_host.address, source_host.port as u16);
        let dest_client = NodeClient::new(&target_host.address, target_host.port as u16);

        // Build the VmConfig proto for the destination node.  We reuse the
        // same fields built for create_vm, but rebuild the bridge references
        // for the destination host.
        let vm_config = crate::grpc_client::node::VmConfig {
            vm_id: vm_id.to_string(),
            cpus: Some(crate::grpc_client::node::CpusConfig {
                boot_vcpus: create_req.boot_vcpus,
                max_vcpus: create_req.max_vcpus,
                topology: None,
                kvm_hyperv: None,
                max_phys_bits: None,
            }),
            memory: Some(crate::grpc_client::node::MemoryConfig {
                size: create_req.memory_size,
                hotplug_size: create_req.memory_hotplug_size,
                mergeable: None,
                shared: if create_req.memory_shared {
                    Some(true)
                } else {
                    None
                },
                hugepages: if create_req.memory_hugepages {
                    Some(true)
                } else {
                    None
                },
                hugepage_size: None,
                prefault: None,
                thp: None,
            }),
            payload: Some(crate::grpc_client::node::PayloadConfig {
                kernel: create_req.kernel.filter(|s| !s.is_empty()),
                cmdline: create_req.cmdline.filter(|s| !s.is_empty()),
                initramfs: create_req.initramfs.filter(|s| !s.trim().is_empty()),
                firmware: create_req.firmware.filter(|s| !s.is_empty()),
            }),
            disks: create_req.disks,
            networks: create_req.networks,
            rng: None,
            serial: Some(crate::grpc_client::node::ConsoleConfig {
                mode: 1, // CONSOLE_MODE_PTY
                file: None,
                socket: None,
                iommu: None,
            }),
            console: None,
            rate_limit_groups: vec![],
            fs: create_req.fs_configs,
            cloud_init: create_req.cloud_init_user_data.as_ref().map(|user_data| {
                crate::grpc_client::node::CloudInitConfig {
                    user_data: user_data.clone(),
                    meta_data: create_req.cloud_init_meta_data.clone().unwrap_or_default(),
                    network_config: create_req
                        .cloud_init_network_config
                        .clone()
                        .unwrap_or_default(),
                }
            }),
            devices: create_req.devices,
            vsock: create_req.vsock,
            // NUMA placement is not carried across migration — the destination node
            // will handle its own NUMA topology.
            numa_placement: None,
        };

        // Step 1: Prepare the destination node.
        let receiver_url = match dest_client.receive_migration(vm_id, vm_config, 0).await {
            Ok(url) => url,
            Err(e) => {
                let msg = format!("receive_migration failed: {:#}", e);
                tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
                let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
                let _ = vms::update_status(&db_pool, vm_id, original_status.clone()).await;
                return;
            }
        };

        let _ = jobs::update_progress(&db_pool, job_id, 25).await;

        // Replace 0.0.0.0 with the destination host's real IP so the source
        // CH can connect to it.
        let actual_receiver_url = receiver_url.replace("0.0.0.0", &target_host.address);

        // Step 2: Initiate the migration on the source node.
        if let Err(e) = source_client
            .send_migration(vm_id, &actual_receiver_url)
            .await
        {
            let msg = format!("send_migration failed: {:#}", e);
            tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
            let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
            let _ = vms::update_status(&db_pool, vm_id, original_status).await;
            return;
        }

        let _ = jobs::update_progress(&db_pool, job_id, 75).await;

        // Step 3: Update the DB to point to the destination host.
        if let Err(e) = vms::update_host_id(&db_pool, vm_id, target_host.id).await {
            tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %e, "Failed to update host_id after migration");
        }
        let _ = vms::update_status(&db_pool, vm_id, original_status).await;

        // Step 4: Clean up source node resources (TAP devices, persisted config).
        // delete_vm on the source is best-effort — the CH process has already
        // exited after migration, so shutdown/kill will fail gracefully.
        if let Err(e) = source_client.delete_vm(vm_id).await {
            tracing::warn!(
                vm_id = %vm_id,
                error = %e,
                "Source cleanup (delete_vm) failed after migration — manual cleanup may be needed"
            );
        }

        let _ = jobs::mark_completed(&db_pool, job_id, None).await;
        tracing::info!(vm_id = %vm_id, job_id = %job_id, "VM migration completed successfully");
    });

    use axum::response::IntoResponse as _;
    Ok(ApiResponse {
        data: VmMigrateResponse { job_id },
        code: StatusCode::ACCEPTED,
    }
    .into_response())
}

/// Request body for `PUT /vms/{vm_id}/resize`.
///
/// At least one of `desired_vcpus` or `desired_ram` must be provided.
/// - `desired_vcpus` must be in the range `[boot_vcpus, max_vcpus]`.
/// - `desired_ram` must be in the range `[memory_size, memory_size + memory_hotplug_size]`.
/// - On x86_64, Cloud Hypervisor ACPI memory hotplug only supports 128 MiB increments.
#[derive(Debug, Deserialize, ToSchema)]
pub struct VmResizeRequest {
    /// Target vCPU count
    pub desired_vcpus: Option<i32>,
    /// Target memory size in bytes
    pub desired_ram: Option<i64>,
}

#[utoipa::path(
    put,
    path = "/vms/{vm_id}/resize",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    request_body = VmResizeRequest,
    responses(
        (status = 200, description = "VM resized successfully", body = Vm),
        (status = 404, description = "VM not found"),
        (status = 422, description = "VM not running, or resize parameters out of range"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn resize_vm(
    Extension(env): Extension<App>,
    Path(vm_id): Path<Uuid>,
    Json(req): Json<VmResizeRequest>,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse as _;
    const HOTPLUG_MEMORY_INCREMENT_BYTES: i64 = 128 * 1024 * 1024;

    if req.desired_vcpus.is_none() && req.desired_ram.is_none() {
        return Err(crate::errors::Error::UnprocessableEntity(
            "At least one of desired_vcpus or desired_ram must be provided".into(),
        ));
    }

    let vm = vms::get(env.pool(), vm_id).await?;
    if vm.status != VmStatus::Running {
        return Err(crate::errors::Error::UnprocessableEntity(
            "VM must be running to resize CPU or memory".into(),
        ));
    }

    if let Some(vcpus) = req.desired_vcpus {
        let valid_range = vm.boot_vcpus..=vm.max_vcpus;
        if !valid_range.contains(&vcpus) {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "desired_vcpus {} is out of range [{}, {}]",
                vcpus, vm.boot_vcpus, vm.max_vcpus
            )));
        }
    }

    if let Some(ram) = req.desired_ram {
        let max_ram = vm.memory_size + vm.memory_hotplug_size.unwrap_or(0);
        let valid_range = vm.memory_size..=max_ram;
        if !valid_range.contains(&ram) {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "desired_ram {} is out of range [{}, {}]",
                ram, vm.memory_size, max_ram
            )));
        }

        let hotplug_addition = ram - vm.memory_size;
        if hotplug_addition % HOTPLUG_MEMORY_INCREMENT_BYTES != 0 {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "desired_ram {} requires a hotplug addition of {} bytes, which is not a multiple of {} bytes",
                ram, hotplug_addition, HOTPLUG_MEMORY_INCREMENT_BYTES
            )));
        }
    }

    let host = host_for_vm(&env, vm_id).await?;
    ensure_resize_capacity(&env, &vm, &host, req.desired_vcpus, req.desired_ram).await?;
    NodeClient::new(&host.address, host.port as u16)
        .resize_vm(vm_id, req.desired_vcpus, req.desired_ram)
        .await
        .map_err(|e| {
            error!("Failed to resize VM {}: {}", vm_id, e);
            crate::errors::Error::InternalServerError
        })?;

    vms::update_resize(env.pool(), vm_id, req.desired_vcpus, req.desired_ram).await?;

    // Update the already-fetched vm struct rather than doing another SELECT
    let mut updated_vm = vm;
    if let Some(vcpus) = req.desired_vcpus {
        updated_vm.boot_vcpus = vcpus;
    }
    if let Some(ram) = req.desired_ram {
        updated_vm.memory_size = ram;
    }
    Ok(ApiResponse {
        data: updated_vm,
        code: StatusCode::OK,
    }
    .into_response())
}

/// Request body for `PUT /vms/{vm_id}/disks/{disk_id}/resize`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct DiskResizeRequest {
    /// New disk size in bytes. Must be larger than the current size and a multiple of 1 MiB.
    pub new_size_bytes: i64,
}

#[utoipa::path(
    put,
    path = "/vms/{vm_id}/disks/{disk_id}/resize",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier"),
        ("disk_id" = String, Path, description = "Logical disk name (e.g. \"rootfs\", \"disk0\")")
    ),
    request_body = DiskResizeRequest,
    responses(
        (status = 200, description = "Disk resized successfully", body = crate::model::storage_objects::StorageObject),
        (status = 404, description = "VM or disk not found"),
        (status = 422, description = "VM not stopped, disk not resizable, or size invalid"),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn resize_disk(
    Extension(env): Extension<App>,
    Path((vm_id, disk_id)): Path<(Uuid, String)>,
    Json(req): Json<DiskResizeRequest>,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse as _;
    const MIB: i64 = 1024 * 1024;

    let vm = vms::get(env.pool(), vm_id).await?;
    match vm.status {
        VmStatus::Created | VmStatus::Shutdown => {}
        _ => {
            return Err(crate::errors::Error::UnprocessableEntity(
                "VM must be stopped (Created or Shutdown) to resize a disk".into(),
            ));
        }
    }

    let disk = vm_disks::get_by_logical_name(env.pool(), vm_id, &disk_id)
        .await?
        .ok_or(crate::errors::Error::NotFound)?;

    if disk.vhost_user {
        return Err(crate::errors::Error::UnprocessableEntity(
            "vhost-user disks cannot be resized".into(),
        ));
    }

    let so_id = disk.storage_object_id.ok_or_else(|| {
        crate::errors::Error::UnprocessableEntity("disk has no backing storage object".into())
    })?;

    let obj = storage_objects::get(env.pool(), so_id).await?;
    let pool_record = storage_pools::get(env.pool(), obj.storage_pool_id).await?;
    let target = resolve_disk_resize_target(&disk, &obj, &pool_record)?;

    if req.new_size_bytes <= obj.size_bytes {
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "new_size_bytes {} must be greater than current size {}",
            req.new_size_bytes, obj.size_bytes
        )));
    }
    if req.new_size_bytes % MIB != 0 {
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "new_size_bytes {} must be a multiple of 1 MiB ({})",
            req.new_size_bytes, MIB
        )));
    }

    let host = host_for_vm(&env, vm_id).await?;
    NodeClient::new(&host.address, host.port as u16)
        .resize_disk(vm_id, &disk.logical_name, &target.path, req.new_size_bytes)
        .await
        .map_err(|e| {
            error!("Failed to resize disk {} for VM {}: {}", disk_id, vm_id, e);
            crate::errors::Error::InternalServerError
        })?;

    storage_objects::update_size_bytes(env.pool(), target.storage_object_id, req.new_size_bytes)
        .await?;
    let updated_obj = storage_objects::get(env.pool(), target.storage_object_id).await?;

    Ok(ApiResponse {
        data: updated_obj,
        code: StatusCode::OK,
    }
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        storage_objects::StorageObject,
        storage_pools::{StoragePool, StoragePoolStatus, StoragePoolType},
        vm_disks::VmDisk,
    };
    use uuid::Uuid;

    fn make_disk(logical_name: &str) -> VmDisk {
        VmDisk {
            id: Uuid::new_v4(),
            vm_id: Uuid::new_v4(),
            storage_object_id: None,
            logical_name: logical_name.to_string(),
            device_path: format!("/dev/{}", logical_name),
            boot_order: None,
            read_only: false,
            direct: false,
            vhost_user: false,
            vhost_socket: None,
            num_queues: 1,
            queue_size: 128,
            rate_limiter: None,
            rate_limit_group: None,
            pci_segment: 0,
            serial_number: None,
            config: serde_json::json!({}),
            upper_storage_object_id: None,
        }
    }

    fn make_storage_object(
        object_type: StorageObjectType,
        config: serde_json::Value,
    ) -> StorageObject {
        StorageObject {
            id: Uuid::new_v4(),
            name: "obj".to_string(),
            storage_pool_id: Uuid::new_v4(),
            object_type,
            size_bytes: 1024,
            config,
            parent_id: None,
        }
    }

    fn make_storage_pool(pool_type: StoragePoolType) -> StoragePool {
        StoragePool {
            id: Uuid::new_v4(),
            name: "pool".to_string(),
            pool_type,
            status: StoragePoolStatus::Active,
            config: serde_json::json!({}),
            capacity_bytes: None,
            allocated_bytes: None,
        }
    }

    #[test]
    fn next_disk_id_empty() {
        assert_eq!(next_disk_id(&[]), "disk0");
    }

    #[test]
    fn next_disk_id_skips_existing() {
        let existing = vec![make_disk("disk0"), make_disk("disk1")];
        assert_eq!(next_disk_id(&existing), "disk2");
    }

    #[test]
    fn next_disk_id_fills_gap() {
        let existing = vec![make_disk("disk0"), make_disk("disk2")];
        assert_eq!(next_disk_id(&existing), "disk1");
    }

    #[test]
    fn next_disk_id_ignores_non_disk_names() {
        let existing = vec![make_disk("vda"), make_disk("rootfs")];
        assert_eq!(next_disk_id(&existing), "disk0");
    }

    #[test]
    #[allow(clippy::unnecessary_literal_unwrap)]
    fn boot_mode_default_is_kernel() {
        let mode: Option<BootMode> = None;
        assert_eq!(mode.unwrap_or(BootMode::Kernel), BootMode::Kernel);
    }

    #[test]
    fn boot_mode_serde_roundtrip() {
        let json = serde_json::to_string(&BootMode::Firmware).unwrap();
        assert_eq!(json, r#""firmware""#);
        let parsed: BootMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, BootMode::Firmware);

        let json = serde_json::to_string(&BootMode::Kernel).unwrap();
        assert_eq!(json, r#""kernel""#);
        let parsed: BootMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, BootMode::Kernel);
    }

    #[test]
    fn overlaybd_upper_paths_reads_config() {
        let upper_object = make_storage_object(
            StorageObjectType::OverlaybdUpper,
            serde_json::json!({
                "upper_data": "/var/lib/qarax/pools/upper.data",
                "upper_index": "/var/lib/qarax/pools/upper.index"
            }),
        );

        let (upper_data_path, upper_index_path) = overlaybd_upper_paths(Some(&upper_object));

        assert_eq!(
            upper_data_path.as_deref(),
            Some("/var/lib/qarax/pools/upper.data")
        );
        assert_eq!(
            upper_index_path.as_deref(),
            Some("/var/lib/qarax/pools/upper.index")
        );
    }

    #[test]
    fn resolve_disk_resize_target_accepts_local_disk() {
        let disk = make_disk("disk0");
        let object = make_storage_object(
            StorageObjectType::Disk,
            serde_json::json!({ "path": "/var/lib/qarax/disk.raw" }),
        );
        let pool = make_storage_pool(StoragePoolType::Local);

        let target = resolve_disk_resize_target(&disk, &object, &pool).unwrap();

        assert_eq!(target.storage_object_id, object.id);
        assert_eq!(target.path, "/var/lib/qarax/disk.raw");
    }

    #[test]
    fn resolve_disk_resize_target_rejects_persistent_overlaybd() {
        let mut disk = make_disk("disk0");
        disk.upper_storage_object_id = Some(Uuid::new_v4());
        let object = make_storage_object(
            StorageObjectType::OciImage,
            serde_json::json!({
                "image_ref": "registry:5000/test/busybox:latest",
                "registry_url": "http://registry:5000"
            }),
        );
        let pool = make_storage_pool(StoragePoolType::OverlayBd);

        let err = resolve_disk_resize_target(&disk, &object, &pool).unwrap_err();

        assert!(
            matches!(err, crate::errors::Error::UnprocessableEntity(message) if message == "persistent OverlayBD disk resize is not supported yet")
        );
    }
}
