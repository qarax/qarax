use std::collections::HashMap;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::{Extension, Json, extract::Path, response::IntoResponse};
use futures::{SinkExt, StreamExt};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{error, info, instrument, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    App,
    grpc_client::{
        CreateVmRequest, NodeClient, net_configs_from_db,
        node::{DiskConfig, FsConfig, VhostMode},
    },
    model::{
        boot_sources, hosts,
        hosts::Host,
        jobs::{self, JobType, NewJob},
        network_interfaces, networks, snapshots,
        snapshots::{NewSnapshot, Snapshot, SnapshotStatus},
        storage_objects, storage_pools,
        storage_pools::OverlayBdPoolConfig,
        vm_disks::{self, NewVmDisk},
        vm_filesystems::{self, NewVmFilesystem},
        vms::{self, BootMode, NewVm, Vm, VmStatus},
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

/// Pick an UP host with sufficient resources for scheduling a new VM.
async fn pick_host(env: &App, requested_memory: i64) -> Result<Host> {
    hosts::pick_up_host(env.pool(), requested_memory)
        .await?
        .ok_or_else(|| {
            crate::errors::Error::UnprocessableEntity(
                "no hosts in UP state with sufficient resources available for scheduling".into(),
            )
        })
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

#[utoipa::path(
    get,
    path = "/vms",
    responses(
        (status = 200, description = "List all VMs", body = Vec<Vm>),
        (status = 500, description = "Internal server error")
    ),
    tag = "vms"
)]
#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<Vm>>> {
    let hosts = vms::list(env.pool()).await?;
    Ok(ApiResponse {
        data: hosts,
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
    // If an OCI image_ref is provided, use the async job path
    if vm.image_ref.is_some() {
        return create_with_image(env, vm).await;
    }

    // Synchronous path (no image_ref): only write to DB, pick host, store networks.
    // The node is NOT contacted here. create_vm will be called lazily at vm start.
    let mut tx = env.pool().begin().await?;
    let id = vms::create_tx(&mut tx, &vm).await?;

    // Store network interfaces in DB (inside tx, so rolls back if any insert fails)
    for net in vm.networks.as_deref().unwrap_or(&[]) {
        network_interfaces::create(&mut tx, id, net)
            .await
            .map_err(crate::errors::Error::Sqlx)?;
    }

    tx.commit().await?;

    // For any explicit networks with a static IP + network_id, register in IPAM so
    // the IP is tracked and won't be re-allocated to another VM.
    register_static_ips(env.pool(), id, vm.networks.as_deref()).await?;

    // Pick a host and record it
    let host = pick_host(&env, vm.memory_size).await?;
    let _ = vms::update_host_id(env.pool(), id, host.id).await;

    // If network_id is provided, allocate an IP and create a default network interface
    if let Some(network_id) = vm.network_id {
        let ip = networks::next_available_ip(env.pool(), network_id)
            .await
            .map_err(crate::errors::Error::Sqlx)?
            .ok_or_else(|| {
                crate::errors::Error::UnprocessableEntity("No available IPs in network".into())
            })?;

        networks::allocate_ip(env.pool(), network_id, &ip, Some(id))
            .await
            .map_err(crate::errors::Error::Sqlx)?;

        // Create a default network interface for this VM
        let net = crate::model::vms::NewVmNetwork {
            id: "net0".to_string(),
            network_id: Some(network_id),
            mac: None,
            tap: None,
            ip: Some(ip),
            mask: None,
            mtu: None,
            host_mac: None,
            interface_type: None,
            vhost_user: None,
            vhost_socket: None,
            vhost_mode: None,
            num_queues: None,
            queue_size: None,
            rate_limiter: None,
            offload_tso: None,
            offload_ufo: None,
            offload_csum: None,
            pci_segment: None,
            iommu: None,
        };
        let mut net_tx = env.pool().begin().await?;
        network_interfaces::create(&mut net_tx, id, &net)
            .await
            .map_err(crate::errors::Error::Sqlx)?;
        net_tx.commit().await?;
    }

    Ok(ApiResponse {
        data: id.to_string(),
        code: StatusCode::CREATED,
    }
    .into_response())
}

/// Async path: pull OCI image and create VM in a background job, return 202 immediately.
async fn create_with_image(env: App, vm: NewVm) -> Result<axum::response::Response> {
    let image_ref = vm
        .image_ref
        .clone()
        .expect("image_ref checked before calling");

    // Pick host eagerly so we return 422 immediately if none are UP
    let host = pick_host(&env, vm.memory_size).await?;

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
                let vm_defaults = env.vm_defaults();
                (
                    vm_defaults.kernel.clone(),
                    vm_defaults.initramfs.clone().filter(|s| !s.is_empty()),
                    vm_defaults.cmdline.clone(),
                )
            };
        }
        BootMode::Firmware => {
            let d = env.vm_defaults();
            let firmware = d.firmware.clone().filter(|s| !s.is_empty());
            if firmware.is_none() {
                return Err(crate::errors::Error::UnprocessableEntity(
                    "firmware boot mode requires a firmware path (set vm_defaults.firmware or VM_FIRMWARE env var)".into(),
                ));
            }
        }
    }

    // Create VM row with PENDING status
    let mut tx = env.pool().begin().await?;
    let vm_id = vms::create_tx_with_status(&mut tx, &vm, VmStatus::Pending).await?;

    // Store network interfaces in the transaction
    for net in vm.networks.as_deref().unwrap_or(&[]) {
        network_interfaces::create(&mut tx, vm_id, net)
            .await
            .map_err(crate::errors::Error::Sqlx)?;
    }

    tx.commit().await?;

    // For any explicit networks with a static IP + network_id, register in IPAM.
    register_static_ips(env.pool(), vm_id, vm.networks.as_deref()).await?;

    // If network_id is provided, allocate an IP and create a default interface.
    // This mirrors the synchronous create path so async image-based VMs get managed networking.
    if let Some(network_id) = vm.network_id {
        let ip = networks::next_available_ip(env.pool(), network_id)
            .await
            .map_err(crate::errors::Error::Sqlx)?
            .ok_or_else(|| {
                crate::errors::Error::UnprocessableEntity("No available IPs in network".into())
            })?;

        networks::allocate_ip(env.pool(), network_id, &ip, Some(vm_id))
            .await
            .map_err(crate::errors::Error::Sqlx)?;

        let net = crate::model::vms::NewVmNetwork {
            id: "net0".to_string(),
            network_id: Some(network_id),
            mac: None,
            tap: None,
            ip: Some(ip),
            mask: None,
            mtu: None,
            host_mac: None,
            interface_type: None,
            vhost_user: None,
            vhost_socket: None,
            vhost_mode: None,
            num_queues: None,
            queue_size: None,
            rate_limiter: None,
            offload_tso: None,
            offload_ufo: None,
            offload_csum: None,
            pci_segment: None,
            iommu: None,
        };
        let mut net_tx = env.pool().begin().await?;
        network_interfaces::create(&mut net_tx, vm_id, &net)
            .await
            .map_err(crate::errors::Error::Sqlx)?;
        net_tx.commit().await?;
    }

    // Record host assignment
    let _ = vms::update_host_id(env.pool(), vm_id, host.id).await;

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

/// Helper to register explicitly provided static IPs in IPAM during VM creation.
async fn register_static_ips(
    pool: &sqlx::PgPool,
    vm_id: Uuid,
    networks: Option<&[crate::model::vms::NewVmNetwork]>,
) -> Result<()> {
    for net in networks.unwrap_or(&[]) {
        if let (Some(net_id), Some(ip)) = (net.network_id, &net.ip) {
            networks::allocate_ip(pool, net_id, ip, Some(vm_id))
                .await
                .map_err(crate::errors::Error::Sqlx)?;
        }
    }
    Ok(())
}

/// Background task for the virtiofs (Nydus) OCI image boot path.
#[allow(clippy::too_many_arguments)]
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

    let disk_record = NewVmDisk {
        vm_id,
        storage_object_id: Some(so_id),
        logical_name: logical_name.clone(),
        device_path: format!("/dev/{}", logical_name),
        boot_order: Some(0),
        read_only: Some(false),
        direct: None,
        vhost_user: None,
        vhost_socket: None,
        num_queues: None,
        queue_size: None,
        rate_limiter: None,
        rate_limit_group: None,
        pci_segment: None,
        serial_number: None,
        config: serde_json::Value::default(),
    };
    if let Err(e) = vm_disks::create(db_pool, &disk_record).await {
        let msg = format!("Failed to persist OverlayBD disk record: {}", e);
        tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
        if let Err(cleanup_err) = storage_objects::delete(db_pool, so_id).await {
            tracing::warn!(vm_id = %vm_id, storage_object_id = %so_id, error = %cleanup_err, "Failed to clean up orphaned storage object");
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
    let vm = vms::get(env.pool(), vm_id).await?;
    let original_status = vm.status.clone();

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
        VmStatus::Created | VmStatus::Shutdown | VmStatus::Unknown => {
            // Valid states to start from
        }
    }

    let host = host_for_vm(&env, vm_id).await?;

    // Set VM status to Pending to prevent double-start
    vms::update_status(env.pool(), vm_id, VmStatus::Pending).await?;

    // Build create request eagerly (before spawning) so we can return errors synchronously
    let create_req = if original_status == VmStatus::Created {
        Some(build_create_vm_request(&env, &vm).await.map_err(|e| {
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

        if let Err(e) = jobs::mark_running(&db_pool, job_id).await {
            tracing::error!(job_id = %job_id, error = %e, "Failed to mark job as running");
            return;
        }

        let node_client = NodeClient::new(&host.address, host.port as u16);

        // For a VM in Created state, call create_vm first
        if let Some(req) = create_req {
            if let Err(e) = node_client.create_vm(req).await {
                let msg = format!("create_vm failed: {:#}", e);
                tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
                let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
                let _ = vms::update_status(&db_pool, vm_id, original_status).await;
                return;
            }
            let _ = jobs::update_progress(&db_pool, job_id, 50).await;
        }

        if let Err(e) = node_client.start_vm(vm_id).await {
            let msg = format!("start_vm failed: {:#}", e);
            tracing::error!(vm_id = %vm_id, job_id = %job_id, error = %msg);
            let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
            let _ = vms::update_status(&db_pool, vm_id, original_status).await;
            return;
        }

        let _ = vms::update_status(&db_pool, vm_id, VmStatus::Running).await;
        let _ = jobs::mark_completed(&db_pool, job_id, None).await;

        tracing::info!(vm_id = %vm_id, job_id = %job_id, "VM start job completed");
    });

    use axum::response::IntoResponse as _;
    Ok(ApiResponse {
        data: VmStartResponse { job_id },
        code: StatusCode::ACCEPTED,
    }
    .into_response())
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
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
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
) -> Result<ApiResponse<Vec<Snapshot>>> {
    // Verify VM exists
    vms::get(env.pool(), vm_id).await?;

    let list = snapshots::list_for_vm(env.pool(), vm_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;

    Ok(ApiResponse {
        data: list,
        code: StatusCode::OK,
    })
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSnapshotRequest {
    /// Human-readable name for the snapshot (auto-generated if omitted).
    pub name: Option<String>,
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

    let snapshot_id = Uuid::new_v4();
    let snapshot_url = format!("file://{}/{}/{}", env.snapshot_dir(), vm_id, snapshot_id);
    let name = body
        .name
        .unwrap_or_else(|| format!("snapshot-{}", &snapshot_id.to_string()[..8]));

    let id = snapshots::create(
        env.pool(),
        &NewSnapshot {
            vm_id,
            name,
            snapshot_url: snapshot_url.clone(),
        },
    )
    .await
    .map_err(crate::errors::Error::Sqlx)?;

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
            snapshots::update_status(env.pool(), id, SnapshotStatus::Ready)
                .await
                .map_err(crate::errors::Error::Sqlx)?;

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

    let snapshot = snapshots::get(env.pool(), id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;

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
        VmStatus::Shutdown | VmStatus::Created | VmStatus::Unknown => {}
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

    // The node handles the full restore flow: kills any existing CH process,
    // spawns a fresh one, and calls vm.restore directly (no vm.create needed).
    if let Err(e) = node_client.restore_vm(vm_id, &snapshot.snapshot_url).await {
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

/// Build a `CreateVmRequest` from DB state — called at `vm start` for lazily-provisioned VMs.
async fn build_create_vm_request(env: &App, vm: &Vm) -> Result<CreateVmRequest> {
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

    let vm_id = vm.id;

    // Resolve boot payload based on boot_mode
    let (kernel, firmware, initramfs, default_cmdline) = match vm.boot_mode {
        BootMode::Firmware => {
            let d = env.vm_defaults();
            let fw = d.firmware.clone().filter(|s| !s.is_empty()).ok_or_else(|| {
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
                    d.kernel.clone(),
                    d.initramfs.clone().filter(|s| !s.is_empty()),
                    d.cmdline.clone(),
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

    // Batch-fetch storage objects and pools to avoid N+1 queries
    let so_ids: Vec<Uuid> = db_disks
        .iter()
        .filter_map(|d| d.storage_object_id)
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

                    resolved_disks.push(DiskConfig {
                        id: disk.logical_name.clone(),
                        path: None,
                        readonly: Some(disk.read_only),
                        direct: if disk.direct { Some(true) } else { None },
                        vhost_user: if disk.vhost_user { Some(true) } else { None },
                        vhost_socket: disk.vhost_socket.clone(),
                        num_queues: Some(disk.num_queues),
                        queue_size: Some(disk.queue_size),
                        rate_limiter: None,
                        rate_limit_group: disk.rate_limit_group.clone(),
                        pci_segment: if disk.pci_segment != 0 {
                            Some(disk.pci_segment)
                        } else {
                            None
                        },
                        serial: disk.serial_number.clone(),
                        oci_image_ref: Some(image_ref),
                        registry_url: Some(registry_url),
                    });
                    has_overlaybd_boot = true;
                }
                storage_pools::StoragePoolType::Local | storage_pools::StoragePoolType::Nfs => {
                    let path = storage_objects::get_path_from_config(&obj.config);
                    resolved_disks.push(DiskConfig {
                        id: disk.logical_name.clone(),
                        path,
                        readonly: Some(disk.read_only),
                        direct: if disk.direct { Some(true) } else { None },
                        vhost_user: if disk.vhost_user { Some(true) } else { None },
                        vhost_socket: disk.vhost_socket.clone(),
                        num_queues: Some(disk.num_queues),
                        queue_size: Some(disk.queue_size),
                        rate_limiter: None,
                        rate_limit_group: disk.rate_limit_group.clone(),
                        pci_segment: if disk.pci_segment != 0 {
                            Some(disk.pci_segment)
                        } else {
                            None
                        },
                        serial: disk.serial_number.clone(),
                        oci_image_ref: None,
                        registry_url: None,
                    });
                }
            }
        }
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
        let fs_config = FsConfig {
            tag: fs.tag.clone(),
            socket: socket_path,
            num_queues: fs.num_queues,
            queue_size: fs.queue_size,
            pci_segment: fs.pci_segment,
            id: Some("fs0".to_string()),
            bootstrap_path: None,
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
        disks: resolved_disks,
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
#[utoipa::path(
    post,
    path = "/vms/{vm_id}/disks",
    params(
        ("vm_id" = uuid::Uuid, Path, description = "VM unique identifier")
    ),
    request_body = AttachDiskRequest,
    responses(
        (status = 201, description = "Disk attached", body = crate::model::vm_disks::VmDisk),
        (status = 404, description = "VM or storage object not found"),
        (status = 422, description = "VM not in Created state"),
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
    if vm.status != VmStatus::Created {
        return Err(crate::errors::Error::UnprocessableEntity(
            "Disks can only be linked while the VM is in Created state".into(),
        ));
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
        direct: None,
        vhost_user: None,
        vhost_socket: None,
        num_queues: None,
        queue_size: None,
        rate_limiter: None,
        rate_limit_group: None,
        pci_segment: None,
        serial_number: None,
        config: serde_json::Value::default(),
    };

    let id = vm_disks::create(env.pool(), &disk_record).await?;
    let disk = vm_disks::get(env.pool(), id).await?;

    Ok(ApiResponse {
        data: disk,
        code: StatusCode::CREATED,
    }
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::vm_disks::VmDisk;
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
}
