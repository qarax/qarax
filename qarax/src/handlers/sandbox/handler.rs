use axum::{Extension, Json, extract::Path, response::IntoResponse};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    App,
    errors::Error,
    grpc_client::{
        NodeClient,
        node::{DiskCopy, Fork},
    },
    handlers::{ApiResponse, Result},
    model::{
        hosts::{self, Host, SchedulingRequest},
        jobs::{self, JobType, NewJob},
        network_interfaces, networks, sandbox_pool_members,
        sandboxes::{
            self, CreateSandboxResponse, ExecSandboxRequest, ExecSandboxResponse,
            ForkSandboxRequest, NewSandbox, Sandbox, SandboxStatus,
        },
        security_groups,
        storage_objects::{self, StorageObject, StorageObjectType},
        storage_pools::{self, StoragePoolType},
        vm_disks,
        vms::{self, Vm, VmStatus},
    },
    network_policy,
    pagination::{PageQuery, PageRequest, time_key},
    sandbox_pool_manager,
    sandbox_runtime::{self, ReadyOutcome, ReadyWait, destroy_vm, resolve_sandbox_vm},
};

use super::super::vm::handler::{
    create_vm_internal, create_vm_with_image_internal, net_configs_on_host, start_vm_internal,
};

#[utoipa::path(
    post,
    path = "/v1/sandboxes",
    request_body = NewSandbox,
    responses(
        (status = 202, description = "Sandbox creation started", body = CreateSandboxResponse),
        (status = 422, description = "Invalid input or no available hosts"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandboxes"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(req): Json<NewSandbox>,
) -> Result<axum::response::Response> {
    let idle_timeout_secs = req
        .idle_timeout_secs
        .unwrap_or(env.sandbox().default_idle_timeout_secs);
    if let Some(response) = try_claim_prewarmed_sandbox(&env, &req, idle_timeout_secs).await? {
        return Ok(ApiResponse {
            data: response,
            code: StatusCode::ACCEPTED,
        }
        .into_response());
    }

    let resolved_vm = resolve_sandbox_vm(&env, &req).await?;

    let (vm_id, image_pull_job) = if resolved_vm.image_ref.is_some() {
        let (vm_id, job_id) = create_vm_with_image_internal(&env, resolved_vm).await?;
        (vm_id, Some(job_id))
    } else {
        (create_vm_internal(&env, resolved_vm).await?, None)
    };

    // Create the sandbox record
    let sandbox_id = Uuid::new_v4();
    if let Err(e) = sandboxes::create(
        env.pool(),
        sandbox_id,
        vm_id,
        Some(req.vm_template_id),
        &req.name,
        idle_timeout_secs,
    )
    .await
    {
        destroy_vm(env.pool(), vm_id).await;
        return Err(crate::errors::Error::Sqlx(e));
    }

    // For OCI: the image-pull job is the user-visible job; start_vm_internal
    // is invoked later by the watcher once the VM transitions to Created.
    // For non-OCI: start the VM eagerly.
    let wait = match image_pull_job {
        Some(job_id) => ReadyWait::PullingImage { job_id },
        None => match start_vm_internal(&env, vm_id).await {
            Ok(job_id) => ReadyWait::Starting { job_id },
            Err(e) => {
                destroy_vm(env.pool(), vm_id).await;
                let _ = sandboxes::delete(env.pool(), sandbox_id).await;
                return Err(e);
            }
        },
    };

    spawn_sandbox_ready_watcher(env.clone(), sandbox_id, vm_id, wait);

    Ok(ApiResponse {
        data: CreateSandboxResponse {
            id: sandbox_id,
            vm_id,
            job_id: wait.job_id(),
        },
        code: StatusCode::ACCEPTED,
    }
    .into_response())
}

#[utoipa::path(
    post,
    path = "/v1/sandboxes/{sandbox_id}/exec",
    params(
        ("sandbox_id" = uuid::Uuid, Path, description = "Sandbox unique identifier")
    ),
    request_body = ExecSandboxRequest,
    responses(
        (status = 200, description = "Command executed inside the sandbox", body = ExecSandboxResponse),
        (status = 404, description = "Sandbox not found"),
        (status = 422, description = "Sandbox is not ready or guest exec is unavailable"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandboxes"
)]
#[instrument(skip(env, req))]
pub async fn exec(
    Extension(env): Extension<App>,
    Path(sandbox_id): Path<Uuid>,
    Json(req): Json<ExecSandboxRequest>,
) -> Result<ApiResponse<ExecSandboxResponse>> {
    if req.command.is_empty() {
        return Err(crate::errors::Error::UnprocessableEntity(
            "command must contain at least one argument".into(),
        ));
    }

    let sandbox = sandboxes::get(env.pool(), sandbox_id).await?;
    if sandbox.status != SandboxStatus::Ready {
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "sandbox {} is not ready",
            sandbox_id
        )));
    }

    let vm = vms::get(env.pool(), sandbox.vm_id).await?;
    if vm.status != VmStatus::Running {
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "sandbox VM {} is not running",
            sandbox.vm_id
        )));
    }

    let host_id = vm.host_id.ok_or_else(|| {
        crate::errors::Error::UnprocessableEntity("sandbox VM has no assigned host".into())
    })?;
    let host = hosts::get_by_id(env.pool(), host_id)
        .await?
        .ok_or_else(|| {
            crate::errors::Error::UnprocessableEntity("assigned host not found".into())
        })?;

    let client = crate::grpc_client::NodeClient::new(&host.address, host.port as u16);
    let response = client
        .exec_vm(sandbox.vm_id, req.command, req.timeout_secs)
        .await
        .map_err(|e| match e.downcast::<crate::errors::Error>() {
            Ok(err) => err,
            Err(err) => {
                tracing::error!(sandbox_id = %sandbox_id, error = %err, "sandbox exec failed");
                crate::errors::Error::InternalServerError
            }
        })?;

    sandboxes::touch_activity(env.pool(), sandbox_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;

    Ok(ApiResponse {
        data: ExecSandboxResponse {
            exit_code: response.exit_code,
            stdout: response.stdout,
            stderr: response.stderr,
            timed_out: response.timed_out,
        },
        code: StatusCode::OK,
    })
}

/// The most forks one request makes.
const MAX_FORKS: u32 = 16;

/// `vms.name` is VARCHAR(50).
const MAX_VM_NAME_CHARS: usize = 50;

/// Fork a running sandbox
///
/// Makes `count` new sandboxes that continue from this one's current state:
/// its memory, its running processes and its files. They run on the same host
/// and are ready when this returns; this one keeps running. Each fork's random
/// generator is reseeded and its clock set. A fork of a sandbox booted from an
/// OCI image gets the image and the sandbox's changes to it as they were, and a
/// copy of each of its writable disks on local and NFS pools, of its own;
/// read-only disks are shared. Each of a fork's network interfaces is on its
/// source's network with an address of its own, and the fork has its source's
/// security groups; a sandbox's cloud-init seed disk is each fork's own. Only
/// sandboxes with the guest agent, no network interface outside a managed
/// bridge or isolated network (nor one whose network config comes from
/// cloud-init) and no writable disk on another kind of pool fork.
#[utoipa::path(
    post,
    path = "/v1/sandboxes/{sandbox_id}/fork",
    params(
        ("sandbox_id" = uuid::Uuid, Path, description = "Sandbox unique identifier")
    ),
    request_body = ForkSandboxRequest,
    responses(
        (status = 201, description = "The forks, ready", body = Vec<Sandbox>),
        (status = 404, description = "Sandbox not found"),
        (status = 422, description = "The sandbox can't be forked, or its host has no room for the forks"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandboxes"
)]
#[instrument(skip(env))]
pub async fn fork(
    Extension(env): Extension<App>,
    Path(sandbox_id): Path<Uuid>,
    Json(req): Json<ForkSandboxRequest>,
) -> Result<ApiResponse<Vec<Sandbox>>> {
    let count = req.count.unwrap_or(1);
    if !(1..=MAX_FORKS).contains(&count) {
        return Err(Error::UnprocessableEntity(format!(
            "count must be between 1 and {MAX_FORKS}"
        )));
    }

    let source = sandboxes::get(env.pool(), sandbox_id).await?;
    if source.status != SandboxStatus::Ready {
        return Err(Error::UnprocessableEntity(format!(
            "sandbox {sandbox_id} is not ready"
        )));
    }
    let vm = vms::get(env.pool(), source.vm_id).await?;
    ensure_forkable(&vm)?;
    let disks = forkable_disks(&env, &vm).await?;
    let nics = forkable_nics(&env, &vm).await?;
    let host_id = vm
        .host_id
        .ok_or_else(|| Error::UnprocessableEntity("sandbox VM has no assigned host".into()))?;

    // The forks' rows, made with the capacity check that keeps their host
    // from taking more than it holds. The VMs stay PENDING, out of the VM
    // monitor's way, until the node has started them.
    let mut tx = env.pool().begin().await?;
    let host = hosts::pick_host_tx(
        &mut tx,
        &SchedulingRequest {
            memory_bytes: vm.memory_size * i64::from(count),
            vcpus: vm.boot_vcpus * count as i32,
            disk_bytes: 0,
            architecture: None,
            storage_pool_id: None,
            required_network_ids: vec![],
            requires_overlaybd: false,
            gpu: None,
            placement_policy: None,
            excluded_host_ids: vec![],
            required_host_id: Some(host_id),
        },
        env.scheduling(),
    )
    .await?
    .ok_or_else(|| {
        Error::UnprocessableEntity(format!(
            "the sandbox's host is not up or has no room for {count} more VMs like it"
        ))
    })?;
    let mut forks = Vec::with_capacity(count as usize);
    let mut copies = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let fork_sandbox_id = Uuid::new_v4();
        let name = fork_name(&source.name, fork_sandbox_id);
        let fork_vm_id = vms::create_fork_tx(&mut tx, vm.id, &name).await?;
        let mut disk_copies = Vec::new();
        for disk in &disks {
            let object_id = match disk {
                ForkDisk::Image { image_id, .. } => {
                    storage_objects::copy_oci_image_for_fork_tx(&mut tx, *image_id, fork_vm_id)
                        .await?
                }
                ForkDisk::Shared { object_id, .. } => *object_id,
                ForkDisk::Copied {
                    drive_id, object, ..
                } => {
                    let copy = storage_objects::create_disk_copy_for_fork_tx(
                        &mut tx,
                        env.pool(),
                        object,
                        format!("fork-{fork_vm_id}-{drive_id}"),
                        fork_vm_id,
                        host.id,
                    )
                    .await?;
                    let path =
                        storage_objects::get_path_from_config(&copy.config).ok_or_else(|| {
                            Error::UnprocessableEntity(format!(
                                "the storage pool of disk {drive_id} has no path for a copy"
                            ))
                        })?;
                    disk_copies.push(DiskCopy {
                        drive_id: drive_id.clone(),
                        path,
                    });
                    copy.id
                }
            };
            vm_disks::copy_for_fork_tx(&mut tx, disk.id(), fork_vm_id, object_id).await?;
        }
        copies.push(disk_copies);
        for nic in &nics {
            let address = networks::allocate_ip_tx(&mut tx, nic.network_id, None, fork_vm_id)
                .await?
                .ok_or_else(|| {
                    Error::UnprocessableEntity(format!(
                        "network {} has no address left for a fork",
                        nic.network_id
                    ))
                })?;
            network_interfaces::copy_for_fork_tx(&mut tx, nic.id, fork_vm_id, &address.ip_address)
                .await?;
        }
        security_groups::copy_for_fork_tx(&mut tx, vm.id, fork_vm_id).await?;
        sandboxes::create_tx_with_status(
            &mut tx,
            fork_sandbox_id,
            fork_vm_id,
            source.vm_template_id,
            &name,
            source.idle_timeout_secs,
            SandboxStatus::Provisioning,
        )
        .await?;
        forks.push((fork_sandbox_id, fork_vm_id));
    }
    tx.commit().await?;
    sandboxes::touch_activity(env.pool(), sandbox_id).await?;

    let fork_vm_ids: Vec<Uuid> = forks.iter().map(|(_, vm_id)| *vm_id).collect();
    if let Err(e) = start_forks(&env, &host, vm.id, &fork_vm_ids, copies).await {
        // The node starts all of the forks or none: no VM is left to stop.
        // Their NICs, addresses, security groups and own disks go with their
        // rows; the files of their disk copies through the cleanup queue.
        for vm_id in &fork_vm_ids {
            match vms::delete(env.pool(), *vm_id).await {
                Ok(deleted) => crate::storage_cleanup::process_now(env.pool(), &deleted).await,
                Err(e) => {
                    tracing::error!(vm_id = %vm_id, error = %e, "Failed to delete the row of a failed fork")
                }
            }
        }
        return Err(e);
    }

    let mut forked = Vec::with_capacity(forks.len());
    for (fork_sandbox_id, fork_vm_id) in forks {
        vms::transition(
            env.pool(),
            fork_vm_id,
            &[VmStatus::Pending],
            VmStatus::Running,
        )
        .await?;
        sandboxes::mark_ready(env.pool(), fork_sandbox_id).await?;
        forked.push(
            sandboxes::get_with_details(env.pool(), fork_sandbox_id)
                .await?
                .into(),
        );
    }

    Ok(ApiResponse {
        data: forked,
        code: StatusCode::CREATED,
    })
}

/// Have `host`'s node start `fork_vm_ids`, whose rows are made, from
/// `source_id`: each with its NICs, security groups and `copies` of the
/// source's data disks.
async fn start_forks(
    env: &App,
    host: &Host,
    source_id: Uuid,
    fork_vm_ids: &[Uuid],
    copies: Vec<Vec<DiskCopy>>,
) -> Result<()> {
    let mut forks = Vec::with_capacity(fork_vm_ids.len());
    for (vm_id, disks) in fork_vm_ids.iter().zip(copies) {
        let nics = network_interfaces::list_by_vm(env.pool(), *vm_id).await?;
        let (networks, _) = net_configs_on_host(env, &nics, Some(host.id)).await?;
        let firewall =
            network_policy::firewall_interfaces_for_vm(env.pool(), *vm_id, host.id).await?;
        forks.push(Fork {
            vm_id: vm_id.to_string(),
            networks,
            firewall,
            disks,
        });
    }
    let client = NodeClient::new(&host.address, host.port as u16);
    client
        .fork_vm(source_id, forks)
        .await
        .map_err(|e| match e.downcast::<Error>() {
            Ok(err) => err,
            Err(err) => {
                tracing::error!(source_vm_id = %source_id, error = %err, "sandbox fork failed");
                Error::InternalServerError
            }
        })
}

/// Only a running VM with the guest agent forks. Its disks are
/// [`forkable_disks`]'s, its NICs [`forkable_nics`]'.
fn ensure_forkable(vm: &Vm) -> Result<()> {
    let refusal = if vm.status != VmStatus::Running {
        format!("sandbox VM {} is not running", vm.id)
    } else if !vm.guest_agent {
        "a sandbox without the guest agent can't be forked: the agent reseeds each fork's \
         random generator"
            .to_string()
    } else {
        return Ok(());
    };
    Err(Error::UnprocessableEntity(refusal))
}

/// A NIC whose copy a fork gets, on `network_id` with an address of its own.
struct ForkNic {
    id: Uuid,
    network_id: Uuid,
}

/// The NICs of `vm` its forks get copies of: each on a managed bridge or
/// isolated network, where the fork gets an address of its own and its guest
/// agent switches the guest to it. Any other NIC is refused, as are NICs whose
/// guest configures them from cloud-init rather than the kernel command line:
/// the forks would keep their source's addresses.
async fn forkable_nics(env: &App, vm: &Vm) -> Result<Vec<ForkNic>> {
    let nics = network_interfaces::list_by_vm(env.pool(), vm.id).await?;
    if !nics.is_empty() && vm.cloud_init_network_config.is_some() {
        return Err(Error::UnprocessableEntity(
            "a sandbox whose network config comes from cloud-init can't be forked with its \
             network interfaces: its forks would keep its addresses"
                .into(),
        ));
    }
    let mut forkable = Vec::new();
    for nic in nics {
        let managed = match nic.network_id {
            Some(network_id) => {
                let network = networks::get(env.pool(), network_id).await?;
                matches!(network.network_type.as_deref(), Some("bridge" | "isolated"))
                    .then_some(network_id)
            }
            None => None,
        };
        match managed {
            Some(network_id) if nic.ip_address.is_some() => forkable.push(ForkNic {
                id: nic.id,
                network_id,
            }),
            _ => {
                return Err(Error::UnprocessableEntity(format!(
                    "a sandbox with a network interface outside a managed bridge or isolated \
                     network can't be forked: its forks would keep its addresses (interface {})",
                    nic.device_id
                )));
            }
        }
    }
    Ok(forkable)
}

/// A disk of a sandbox VM, as its forks get it.
enum ForkDisk {
    /// Its OCI image's: each fork gets the image and the VM's writable layer
    /// as it was, and an image object and a writable layer of its own.
    Image { id: Uuid, image_id: Uuid },
    /// A read-only disk, which the forks share.
    Shared { id: Uuid, object_id: Uuid },
    /// A writable disk on a local or NFS pool: each fork gets a copy of it as
    /// it was, its own.
    Copied {
        id: Uuid,
        drive_id: String,
        object: StorageObject,
    },
}

impl ForkDisk {
    fn id(&self) -> Uuid {
        match self {
            ForkDisk::Image { id, .. }
            | ForkDisk::Shared { id, .. }
            | ForkDisk::Copied { id, .. } => *id,
        }
    }
}

/// The disks of `vm`, as its forks get them. A persistent OverlayBD writable
/// layer belongs to its storage object, a disk outside a storage pool or on
/// another kind of pool can't be copied: the forks would write to the same
/// file. Those are refused.
async fn forkable_disks(env: &App, vm: &Vm) -> Result<Vec<ForkDisk>> {
    let mut forkable = Vec::new();
    for disk in vm_disks::list_by_vm(env.pool(), vm.id).await? {
        if disk.upper_storage_object_id.is_some() {
            return Err(Error::UnprocessableEntity(
                "a sandbox with a persistent disk layer can't be forked: its forks would \
                 write to the same layer"
                    .into(),
            ));
        }
        let object = match disk.storage_object_id {
            Some(id) => storage_objects::get(env.pool(), id).await?,
            None => {
                return Err(Error::UnprocessableEntity(format!(
                    "a sandbox with a disk outside a storage pool can't be forked: its forks \
                     would write to the same file (disk {})",
                    disk.logical_name
                )));
            }
        };
        let fork_disk = if object.object_type == StorageObjectType::OciImage {
            ForkDisk::Image {
                id: disk.id,
                image_id: object.id,
            }
        } else if disk.read_only {
            ForkDisk::Shared {
                id: disk.id,
                object_id: object.id,
            }
        } else {
            let pool = storage_pools::get(env.pool(), object.storage_pool_id).await?;
            if object.object_type != StorageObjectType::Disk
                || !matches!(
                    pool.pool_type,
                    StoragePoolType::Local | StoragePoolType::Nfs
                )
            {
                return Err(Error::UnprocessableEntity(format!(
                    "a sandbox with a writable disk that isn't a disk on a local or NFS pool \
                     can't be forked: its forks would write to the same file (disk {})",
                    disk.logical_name
                )));
            }
            ForkDisk::Copied {
                id: disk.id,
                drive_id: disk.logical_name,
                object,
            }
        };
        forkable.push(fork_disk);
    }
    Ok(forkable)
}

/// `<source>-fork-<8 hex>`, with the source's name cut to fit a VM name.
fn fork_name(source: &str, fork_id: Uuid) -> String {
    let suffix = format!("-fork-{}", &fork_id.simple().to_string()[..8]);
    let source: String = source
        .chars()
        .take(MAX_VM_NAME_CHARS - suffix.len())
        .collect();
    format!("{source}{suffix}")
}

async fn try_claim_prewarmed_sandbox(
    env: &App,
    req: &NewSandbox,
    idle_timeout_secs: i32,
) -> Result<Option<CreateSandboxResponse>> {
    if req.instance_type_id.is_some() || req.network_id.is_some() {
        return Ok(None);
    }

    let mut tx = env
        .pool()
        .begin()
        .await
        .map_err(crate::errors::Error::Sqlx)?;
    let Some(member) =
        sandbox_pool_members::claim_ready_for_template_tx(&mut tx, req.vm_template_id)
            .await
            .map_err(crate::errors::Error::Sqlx)?
    else {
        return Ok(None);
    };

    let sandbox_id = Uuid::new_v4();
    let job = jobs::create_completed_tx(
        &mut tx,
        NewJob {
            job_type: JobType::SandboxClaim,
            description: Some(format!("Claimed prewarmed sandbox {}", req.name)),
            resource_id: Some(sandbox_id),
            resource_type: Some(jobs::resource_types::SANDBOX.to_string()),
        },
        Some(serde_json::json!({ "source": "prewarmed_pool" })),
    )
    .await
    .map_err(crate::errors::Error::Sqlx)?;

    vms::update_name_tx(&mut tx, member.vm_id, &req.name)
        .await
        .map_err(crate::errors::Error::Sqlx)?;
    sandboxes::create_tx_with_status(
        &mut tx,
        sandbox_id,
        member.vm_id,
        Some(req.vm_template_id),
        &req.name,
        idle_timeout_secs,
        SandboxStatus::Ready,
    )
    .await
    .map_err(crate::errors::Error::Sqlx)?;
    sandbox_pool_members::delete_tx(&mut tx, member.id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;
    tx.commit().await.map_err(crate::errors::Error::Sqlx)?;

    let env_for_refill = env.clone();
    let vm_template_id = req.vm_template_id;
    tokio::spawn(async move {
        let _ = sandbox_pool_manager::sync_pool_for_template(&env_for_refill, vm_template_id).await;
    });

    Ok(Some(CreateSandboxResponse {
        id: sandbox_id,
        vm_id: member.vm_id,
        job_id: job.id,
    }))
}

fn spawn_sandbox_ready_watcher(env: App, sandbox_id: Uuid, vm_id: Uuid, wait: ReadyWait) {
    tokio::spawn(async move {
        let outcome = sandbox_runtime::watch_for_ready(&env, vm_id, wait).await;
        let error = match outcome {
            ReadyOutcome::Ready => {
                let _ = sandboxes::mark_ready(env.pool(), sandbox_id).await;
                tracing::info!(sandbox_id = %sandbox_id, vm_id = %vm_id, "Sandbox ready");
                return;
            }
            ReadyOutcome::Failed(msg) => msg,
            ReadyOutcome::TimedOut => "timed out waiting for VM to start".to_string(),
            // VM row vanished; nothing to mark.
            ReadyOutcome::Gone => return,
        };
        tracing::warn!(
            sandbox_id = %sandbox_id,
            vm_id = %vm_id,
            error = %error,
            "Sandbox VM failed to become ready"
        );
        let _ = sandboxes::mark_error(env.pool(), sandbox_id, &error).await;
    });
}

#[utoipa::path(
    get,
    path = "/v1/sandboxes",
    params(PageQuery),
    responses(
        (status = 200, description = "Sandboxes, newest first", body = crate::pagination::SandboxPage),
        (status = 422, description = "Invalid limit or cursor"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandboxes"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(page): axum::extract::Query<PageQuery>,
) -> Result<ApiResponse<crate::pagination::SandboxPage>> {
    let page = PageRequest::try_from(page)?;
    let sandboxes: Vec<Sandbox> = sandboxes::list_with_details(env.pool(), &page)
        .await
        .map_err(crate::errors::Error::Sqlx)?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(ApiResponse {
        data: crate::pagination::SandboxPage::new(sandboxes, &page, |sandbox| {
            (time_key(sandbox.created_at), sandbox.id)
        }),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/v1/sandboxes/{sandbox_id}",
    params(
        ("sandbox_id" = uuid::Uuid, Path, description = "Sandbox unique identifier")
    ),
    responses(
        (status = 200, description = "Sandbox details", body = Sandbox),
        (status = 404, description = "Sandbox not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandboxes"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(sandbox_id): Path<Uuid>,
) -> Result<ApiResponse<Sandbox>> {
    let row = sandboxes::get_with_details(env.pool(), sandbox_id).await?;
    let sandbox: Sandbox = row.into();

    Ok(ApiResponse {
        data: sandbox,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/v1/sandboxes/{sandbox_id}/keepalive",
    params(
        ("sandbox_id" = uuid::Uuid, Path, description = "Sandbox unique identifier")
    ),
    responses(
        (status = 204, description = "Sandbox idle clock reset"),
        (status = 404, description = "Sandbox not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandboxes"
)]
#[instrument(skip(env))]
pub async fn keepalive(
    Extension(env): Extension<App>,
    Path(sandbox_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    // Ensure the sandbox exists so the caller gets 404 instead of a silent no-op.
    let _ = sandboxes::get(env.pool(), sandbox_id).await?;

    sandboxes::touch_activity(env.pool(), sandbox_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::NO_CONTENT,
    })
}

#[utoipa::path(
    delete,
    path = "/v1/sandboxes/{sandbox_id}",
    params(
        ("sandbox_id" = uuid::Uuid, Path, description = "Sandbox unique identifier")
    ),
    responses(
        (status = 204, description = "Sandbox deleted"),
        (status = 404, description = "Sandbox not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandboxes"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(sandbox_id): Path<Uuid>,
) -> Result<ApiResponse<()>> {
    let sandbox = sandboxes::get(env.pool(), sandbox_id).await?;
    let vm_id = sandbox.vm_id;

    // Destroy even when the sandbox was already DESTROYING: a delete that
    // failed half way must be retryable.
    sandboxes::mark_destroying(env.pool(), sandbox_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;

    destroy_vm(env.pool(), vm_id).await;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::NO_CONTENT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fork_name_fits_a_vm_name() {
        let fork_id = Uuid::new_v4();
        let name = fork_name(&"a".repeat(80), fork_id);
        assert_eq!(name.chars().count(), MAX_VM_NAME_CHARS);
        assert!(name.ends_with(&format!("-fork-{}", &fork_id.simple().to_string()[..8])));
        assert_eq!(fork_name("agent", fork_id).len(), "agent".len() + 14);
    }
}
