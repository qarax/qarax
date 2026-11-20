use axum::{Extension, Json, extract::Path, response::IntoResponse};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    App,
    errors::Error,
    grpc_client::NodeClient,
    handlers::{ApiResponse, Result},
    model::{
        hosts::{self, SchedulingRequest},
        jobs::{self, JobType, NewJob},
        network_interfaces, sandbox_pool_members,
        sandboxes::{
            self, CreateSandboxResponse, ExecSandboxRequest, ExecSandboxResponse,
            ForkSandboxRequest, NewSandbox, Sandbox, SandboxStatus,
        },
        storage_objects::{self, StorageObjectType},
        vm_disks,
        vms::{self, Hypervisor, Vm, VmStatus},
    },
    pagination::{PageQuery, PageRequest, time_key},
    sandbox_pool_manager,
    sandbox_runtime::{self, ReadyOutcome, ReadyWait, destroy_vm, resolve_sandbox_vm},
};

use super::super::vm::handler::{
    create_vm_internal, create_vm_with_image_internal, start_vm_internal,
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
/// OCI image gets the image and the sandbox's changes to it as they were.
/// Only Firecracker sandboxes without network interfaces, cloud-init or disks
/// besides their image's fork.
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
    ensure_forkable(&env, &vm).await?;
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
    for _ in 0..count {
        let fork_sandbox_id = Uuid::new_v4();
        let name = fork_name(&source.name, fork_sandbox_id);
        let fork_vm_id = vms::create_fork_tx(&mut tx, vm.id, &name).await?;
        vm_disks::copy_for_fork_tx(&mut tx, vm.id, fork_vm_id).await?;
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
    let client = NodeClient::new(&host.address, host.port as u16);
    if let Err(e) = client.fork_vm(vm.id, &fork_vm_ids).await {
        // The node starts all of the forks or none: no VM is left to stop.
        for vm_id in &fork_vm_ids {
            if let Err(e) = vms::delete(env.pool(), *vm_id).await {
                tracing::error!(vm_id = %vm_id, error = %e, "Failed to delete the row of a failed fork");
            }
        }
        return Err(match e.downcast::<Error>() {
            Ok(err) => err,
            Err(err) => {
                tracing::error!(sandbox_id = %sandbox_id, error = %err, "sandbox fork failed");
                Error::InternalServerError
            }
        });
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

/// A fork shares everything but its memory with its source, so only a VM
/// with nothing else to share forks: no NICs (the forks would have their MAC
/// and IP addresses) and no disks (they would write to the same files).
async fn ensure_forkable(env: &App, vm: &Vm) -> Result<()> {
    let refusal = if vm.status != VmStatus::Running {
        format!("sandbox VM {} is not running", vm.id)
    } else if vm.hypervisor != Hypervisor::Firecracker {
        "only Firecracker sandboxes can be forked".to_string()
    } else if !vm.guest_agent {
        "a sandbox without the guest agent can't be forked: the agent reseeds each fork's \
         random generator"
            .to_string()
    } else if !network_interfaces::list_by_vm(env.pool(), vm.id)
        .await?
        .is_empty()
    {
        "a sandbox with network interfaces can't be forked yet: its forks would have their MAC \
         and IP addresses"
            .to_string()
    } else if vm.cloud_init_user_data.is_some() {
        "a sandbox with cloud-init can't be forked yet: its forks would share its seed disk"
            .to_string()
    } else if let Some(refusal) = disk_refusal(env, vm).await? {
        refusal
    } else {
        return Ok(());
    };
    Err(Error::UnprocessableEntity(refusal))
}

/// Why `vm`'s disks keep it from forking, if they do. Its OCI image disk
/// forks: each fork gets the image and the VM's writable layer as it was, and a
/// writable layer of its own. A persistent writable layer belongs to its
/// storage object, and other disks would have the forks write to the same file.
async fn disk_refusal(env: &App, vm: &Vm) -> Result<Option<String>> {
    for disk in vm_disks::list_by_vm(env.pool(), vm.id).await? {
        if disk.upper_storage_object_id.is_some() {
            return Ok(Some(
                "a sandbox with a persistent disk layer can't be forked: its forks would \
                 write to the same layer"
                    .into(),
            ));
        }
        let image = match disk.storage_object_id {
            Some(id) => Some(storage_objects::get(env.pool(), id).await?),
            None => None,
        };
        if !image.is_some_and(|object| object.object_type == StorageObjectType::OciImage) {
            return Ok(Some(format!(
                "a sandbox with disks besides its OCI image's can't be forked yet: its forks \
                 would write to the same files (disk {})",
                disk.logical_name
            )));
        }
    }
    Ok(None)
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
