use axum::{Extension, Json, extract::Path, response::IntoResponse};
use http::StatusCode;
use sqlx::{Postgres, Transaction};
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
        vms::{self, Hypervisor, Vm, VmStatus},
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
    if let Some(response) = claim_from_pool(&env, &req, idle_timeout_secs).await? {
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
pub(crate) const MAX_FORKS: u32 = 16;

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

    let fork_sandbox_ids: Vec<Uuid> = (0..count).map(|_| Uuid::new_v4()).collect();
    let names: Vec<String> = fork_sandbox_ids
        .iter()
        .map(|id| fork_name(&source.name, *id))
        .collect();
    let mut tx = env.pool().begin().await?;
    let forks = create_forks_tx(&env, &mut tx, &vm, &names).await?;
    for ((fork_sandbox_id, fork_vm_id), name) in
        fork_sandbox_ids.iter().zip(&forks.vm_ids).zip(&names)
    {
        sandboxes::create_tx_with_status(
            &mut tx,
            *fork_sandbox_id,
            *fork_vm_id,
            source.vm_template_id,
            name,
            source.idle_timeout_secs,
            SandboxStatus::Provisioning,
        )
        .await?;
    }
    tx.commit().await?;
    sandboxes::touch_activity(env.pool(), sandbox_id).await?;
    start_forks(&env, forks).await?;

    let mut forked = Vec::with_capacity(fork_sandbox_ids.len());
    for fork_sandbox_id in fork_sandbox_ids {
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

/// Forks whose rows [`create_forks_tx`] made, for [`start_forks`] to start.
pub(crate) struct PendingForks {
    host: Host,
    source_vm_id: Uuid,
    /// The forks' VMs, in the order of the names they were made with.
    pub(crate) vm_ids: Vec<Uuid>,
    copies: Vec<Vec<DiskCopy>>,
}

/// The rows of forks of `vm`, one with each of `names`, made in `tx` with the
/// capacity check that keeps their host from taking more than it holds. Each
/// has a copy of its source's image object and writable data disks, NICs with
/// addresses of their own and its source's security groups. The VMs stay
/// PENDING, out of the VM monitor's way, until [`start_forks`] has started
/// them.
pub(crate) async fn create_forks_tx(
    env: &App,
    tx: &mut Transaction<'_, Postgres>,
    vm: &Vm,
    names: &[String],
) -> Result<PendingForks> {
    ensure_forkable(vm)?;
    let disks = forkable_disks(tx, vm).await?;
    let nics = forkable_nics(tx, vm).await?;
    let host_id = vm
        .host_id
        .ok_or_else(|| Error::UnprocessableEntity("sandbox VM has no assigned host".into()))?;

    let count = names.len();
    let memory_copy_on_write = forks_share_memory(vm);
    let fork_memory = if memory_copy_on_write {
        unmeasured_fork_memory(vm.memory_size, env.scheduling().fork_memory_headroom_ratio)
    } else {
        vm.memory_size
    };
    let host = hosts::pick_host_tx(
        tx,
        &SchedulingRequest {
            memory_bytes: fork_memory * count as i64,
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
    let mut vm_ids = Vec::with_capacity(count);
    let mut copies = Vec::with_capacity(count);
    for name in names {
        let fork_vm_id = vms::create_fork_tx(tx, vm.id, name, memory_copy_on_write).await?;
        let mut disk_copies = Vec::new();
        for disk in &disks {
            let object_id = match disk {
                ForkDisk::Image { image_id, .. } => {
                    storage_objects::copy_oci_image_for_fork_tx(tx, *image_id, fork_vm_id).await?
                }
                ForkDisk::Shared { object_id, .. } => *object_id,
                ForkDisk::Copied {
                    drive_id, object, ..
                } => {
                    let copy = storage_objects::create_disk_copy_for_fork_tx(
                        tx,
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
            vm_disks::copy_for_fork_tx(tx, disk.id(), fork_vm_id, object_id).await?;
        }
        copies.push(disk_copies);
        for nic in &nics {
            let address = networks::allocate_ip_tx(tx, nic.network_id, None, fork_vm_id)
                .await?
                .ok_or_else(|| {
                    Error::UnprocessableEntity(format!(
                        "network {} has no address left for a fork",
                        nic.network_id
                    ))
                })?;
            network_interfaces::copy_for_fork_tx(tx, nic.id, fork_vm_id, &address.ip_address)
                .await?;
        }
        security_groups::copy_for_fork_tx(tx, vm.id, fork_vm_id).await?;
        vm_ids.push(fork_vm_id);
    }
    Ok(PendingForks {
        host,
        source_vm_id: vm.id,
        vm_ids,
        copies,
    })
}

/// Have the forks' node start them, and move their VMs to RUNNING. The node
/// starts all of them or none: when it fails, their rows go, and with them
/// their NICs, addresses, security groups and own disks; the files of their
/// disk copies go through the cleanup queue.
pub(crate) async fn start_forks(env: &App, forks: PendingForks) -> Result<()> {
    if let Err(e) = start_forks_on_node(env, &forks).await {
        for vm_id in &forks.vm_ids {
            match vms::delete(env.pool(), *vm_id).await {
                Ok(deleted) => crate::storage_cleanup::process_now(env.pool(), &deleted).await,
                Err(e) => {
                    tracing::error!(vm_id = %vm_id, error = %e, "Failed to delete the row of a failed fork")
                }
            }
        }
        return Err(e);
    }
    for vm_id in &forks.vm_ids {
        vms::transition(env.pool(), *vm_id, &[VmStatus::Pending], VmStatus::Running).await?;
    }
    Ok(())
}

/// Have the forks' node start them from their source: each with its NICs,
/// security groups and copies of the source's data disks.
async fn start_forks_on_node(env: &App, forks: &PendingForks) -> Result<()> {
    let host = &forks.host;
    let mut node_forks = Vec::with_capacity(forks.vm_ids.len());
    for (vm_id, disks) in forks.vm_ids.iter().zip(&forks.copies) {
        let nics = network_interfaces::list_by_vm(env.pool(), *vm_id).await?;
        let (networks, _) = net_configs_on_host(env, &nics, Some(host.id)).await?;
        let firewall =
            network_policy::firewall_interfaces_for_vm(env.pool(), *vm_id, host.id).await?;
        node_forks.push(Fork {
            vm_id: vm_id.to_string(),
            networks,
            firewall,
            disks: disks.clone(),
        });
    }
    let source_id = forks.source_vm_id;
    let client = NodeClient::new(&host.address, host.port as u16);
    client
        .fork_vm(source_id, node_forks)
        .await
        .map_err(|e| match e.downcast::<Error>() {
            Ok(err) => err,
            Err(err) => {
                tracing::error!(source_vm_id = %source_id, error = %err, "sandbox fork failed");
                Error::InternalServerError
            }
        })
}

/// Whether `vm`'s forks map its memory copy-on-write, sharing its pages until
/// they write them. Cloud Hypervisor restores a VM with shared memory or huge
/// pages with a copy of its memory; Firecracker has neither.
fn forks_share_memory(vm: &Vm) -> bool {
    match vm.hypervisor {
        Hypervisor::Firecracker => true,
        Hypervisor::CloudHv => !(vm.memory_shared || vm.memory_hugepages),
    }
}

/// What a fork whose memory is copy-on-write is charged before its node has
/// measured it: its headroom alone (`vm_memory_charge` in SQL, which rounds
/// the same way).
fn unmeasured_fork_memory(memory_size: i64, headroom_ratio: f64) -> i64 {
    memory_size.min((memory_size as f64 * headroom_ratio).round_ties_even() as i64)
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
async fn forkable_nics(tx: &mut Transaction<'_, Postgres>, vm: &Vm) -> Result<Vec<ForkNic>> {
    let nics = network_interfaces::list_by_vm(tx.as_mut(), vm.id).await?;
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
                let network = networks::get(tx.as_mut(), network_id).await?;
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
async fn forkable_disks(tx: &mut Transaction<'_, Postgres>, vm: &Vm) -> Result<Vec<ForkDisk>> {
    let mut forkable = Vec::new();
    for disk in vm_disks::list_by_vm(tx.as_mut(), vm.id).await? {
        if disk.upper_storage_object_id.is_some() {
            return Err(Error::UnprocessableEntity(
                "a sandbox with a persistent disk layer can't be forked: its forks would \
                 write to the same layer"
                    .into(),
            ));
        }
        let object = match disk.storage_object_id {
            Some(id) => storage_objects::get(tx.as_mut(), id).await?,
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
            let pool = storage_pools::get(tx.as_mut(), object.storage_pool_id).await?;
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

/// A sandbox from the template's pool, when it has one to give: a ready
/// member, or else a fork of its seed; either starts a refill of the pool.
/// `None` when the request asks for what a pool's VMs don't have (an
/// instance type, a network) or the pool has neither: the sandbox boots a VM
/// of its own.
async fn claim_from_pool(
    env: &App,
    req: &NewSandbox,
    idle_timeout_secs: i32,
) -> Result<Option<CreateSandboxResponse>> {
    if req.instance_type_id.is_some() || req.network_id.is_some() {
        return Ok(None);
    }
    let claimed = match claim_ready_member(env, req, idle_timeout_secs).await? {
        Some(claimed) => claimed,
        None => match fork_pool_seed(env, req, idle_timeout_secs).await? {
            Some(forked) => forked,
            None => return Ok(None),
        },
    };
    // The pool makes up for what it handed out.
    let env = env.clone();
    let vm_template_id = req.vm_template_id;
    tokio::spawn(async move {
        let _ = sandbox_pool_manager::sync_pool_for_template(&env, vm_template_id).await;
    });
    Ok(Some(claimed))
}

/// The pool's oldest ready member, renamed as the sandbox and handed to it.
async fn claim_ready_member(
    env: &App,
    req: &NewSandbox,
    idle_timeout_secs: i32,
) -> Result<Option<CreateSandboxResponse>> {
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

    Ok(Some(CreateSandboxResponse {
        id: sandbox_id,
        vm_id: member.vm_id,
        job_id: job.id,
    }))
}

/// A fork of one of the template's pool's seeds made for the request, ready
/// when this returns as a fork is: when the pool forks and a seed runs. It
/// tries the seeds in the order the pool forks its members from them, the one
/// whose host holds the fewest members first; a seed that can't be forked (its
/// host has no room, say) leaves the request to the next. With none left, the
/// sandbox boots a VM of its own, as a pool without seeds leaves it to.
async fn fork_pool_seed(
    env: &App,
    req: &NewSandbox,
    idle_timeout_secs: i32,
) -> Result<Option<CreateSandboxResponse>> {
    let mut started = None;
    for seed in sandbox_pool_members::forkable_seeds(env.pool(), req.vm_template_id).await? {
        let sandbox_id = Uuid::new_v4();
        match start_seed_fork(env, req, idle_timeout_secs, seed.vm_id, sandbox_id).await {
            Ok(vm_id) => {
                started = Some((seed.vm_id, sandbox_id, vm_id));
                break;
            }
            Err(e) => tracing::warn!(
                seed_vm_id = %seed.vm_id,
                vm_template_id = %req.vm_template_id,
                error = %e,
                "Couldn't fork a seed of the sandbox pool for a sandbox"
            ),
        }
    }
    let Some((seed_vm_id, sandbox_id, vm_id)) = started else {
        return Ok(None);
    };
    sandboxes::mark_ready(env.pool(), sandbox_id).await?;
    let job = jobs::create_completed(
        env.pool(),
        NewJob {
            job_type: JobType::SandboxClaim,
            description: Some(format!("Forked sandbox {} from its pool's seed", req.name)),
            resource_id: Some(sandbox_id),
            resource_type: Some(jobs::resource_types::SANDBOX.to_string()),
        },
        Some(serde_json::json!({ "source": "pool_seed", "seed_vm_id": seed_vm_id })),
    )
    .await?;
    Ok(Some(CreateSandboxResponse {
        id: sandbox_id,
        vm_id,
        job_id: job.id,
    }))
}

/// Start the sandbox's VM, a fork of the seed named as the sandbox; its rows
/// go when the node can't start it.
async fn start_seed_fork(
    env: &App,
    req: &NewSandbox,
    idle_timeout_secs: i32,
    seed_vm_id: Uuid,
    sandbox_id: Uuid,
) -> Result<Uuid> {
    let seed = vms::get(env.pool(), seed_vm_id).await?;
    let mut tx = env.pool().begin().await?;
    let forks = create_forks_tx(env, &mut tx, &seed, std::slice::from_ref(&req.name)).await?;
    let vm_id = forks.vm_ids[0];
    sandboxes::create_tx_with_status(
        &mut tx,
        sandbox_id,
        vm_id,
        Some(req.vm_template_id),
        &req.name,
        idle_timeout_secs,
        SandboxStatus::Provisioning,
    )
    .await?;
    tx.commit().await?;
    start_forks(env, forks).await?;
    Ok(vm_id)
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
