use axum::{Extension, Json, extract::Path, response::IntoResponse};
use http::StatusCode;
use tokio::time::{Duration, interval};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    App,
    handlers::{ApiResponse, Result},
    model::{
        hosts,
        jobs::{self, JobType, NewJob},
        network_interfaces, sandbox_pool_members,
        sandboxes::{
            self, CreateSandboxResponse, ExecSandboxRequest, ExecSandboxResponse, NewSandbox,
            Sandbox, SandboxStatus,
        },
        vms::{self, VmStatus},
    },
    sandbox_pool_manager,
    sandbox_runtime::{destroy_vm, resolve_sandbox_vm},
};

use super::super::vm::handler::{create_vm_internal, start_vm_internal};

#[utoipa::path(
    post,
    path = "/sandboxes",
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
    let idle_timeout_secs = req.idle_timeout_secs.unwrap_or(300);
    if let Some(response) = try_claim_prewarmed_sandbox(&env, &req, idle_timeout_secs).await? {
        return Ok(ApiResponse {
            data: response,
            code: StatusCode::ACCEPTED,
        }
        .into_response());
    }

    let resolved_vm = resolve_sandbox_vm(&env, &req).await?;
    let vm_id = create_vm_internal(&env, resolved_vm).await?;

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

    // Kick off async VM start
    let job_id = match start_vm_internal(&env, vm_id).await {
        Ok(job_id) => job_id,
        Err(e) => {
            destroy_vm(env.pool(), vm_id).await;
            let _ = sandboxes::delete(env.pool(), sandbox_id).await;
            return Err(e);
        }
    };

    spawn_sandbox_ready_watcher(env.pool_arc(), sandbox_id, vm_id, job_id);

    Ok(ApiResponse {
        data: CreateSandboxResponse {
            id: sandbox_id,
            vm_id,
            job_id,
        },
        code: StatusCode::ACCEPTED,
    }
    .into_response())
}

#[utoipa::path(
    post,
    path = "/sandboxes/{sandbox_id}/exec",
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

fn spawn_sandbox_ready_watcher(
    db_pool: std::sync::Arc<sqlx::PgPool>,
    sandbox_id: Uuid,
    vm_id: Uuid,
    job_id: Uuid,
) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(2));
        let mut attempts = 0u32;
        loop {
            ticker.tick().await;
            attempts += 1;
            if attempts > 150 {
                tracing::warn!(
                    sandbox_id = %sandbox_id,
                    vm_id = %vm_id,
                    "Timed out waiting for sandbox VM to start"
                );
                let _ = sandboxes::update_status(
                    &db_pool,
                    sandbox_id,
                    SandboxStatus::Error,
                    Some("timed out waiting for VM to start".to_string()),
                )
                .await;
                break;
            }
            match vms::get(&db_pool, vm_id).await {
                Ok(vm) => match vm.status {
                    VmStatus::Running => {
                        let _ = sandboxes::update_status(
                            &db_pool,
                            sandbox_id,
                            SandboxStatus::Ready,
                            None,
                        )
                        .await;
                        tracing::info!(sandbox_id = %sandbox_id, vm_id = %vm_id, "Sandbox ready");
                        break;
                    }
                    VmStatus::Shutdown | VmStatus::Unknown => {
                        let _ = sandboxes::update_status(
                            &db_pool,
                            sandbox_id,
                            SandboxStatus::Error,
                            Some("VM failed to start".to_string()),
                        )
                        .await;
                        tracing::warn!(
                            sandbox_id = %sandbox_id,
                            vm_id = %vm_id,
                            status = ?vm.status,
                            "Sandbox VM entered error state"
                        );
                        break;
                    }
                    _ => {
                        // VmStatus::Created or Pending: start job may still be in-flight.
                        // Check the job status before declaring failure to avoid a race
                        // where the job fails and reverts to Created before the first poll.
                        if let Ok(job) = jobs::get(&db_pool, job_id).await
                            && job.status == jobs::JobStatus::Failed
                        {
                            let msg = job
                                .error
                                .unwrap_or_else(|| "VM failed to start".to_string());
                            tracing::warn!(
                                sandbox_id = %sandbox_id,
                                vm_id = %vm_id,
                                error = %msg,
                                "Sandbox start job failed"
                            );
                            let _ = sandboxes::update_status(
                                &db_pool,
                                sandbox_id,
                                SandboxStatus::Error,
                                Some(msg),
                            )
                            .await;
                            break;
                        }
                    }
                },
                Err(sqlx::Error::RowNotFound) => break,
                Err(e) => {
                    tracing::warn!(
                        sandbox_id = %sandbox_id,
                        vm_id = %vm_id,
                        error = %e,
                        "Sandbox watcher: failed to poll VM status"
                    );
                }
            }
        }
    });
}

#[utoipa::path(
    get,
    path = "/sandboxes",
    responses(
        (status = 200, description = "List all sandboxes", body = Vec<Sandbox>),
        (status = 500, description = "Internal server error")
    ),
    tag = "sandboxes"
)]
#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<Sandbox>>> {
    let rows = sandboxes::list(env.pool())
        .await
        .map_err(crate::errors::Error::Sqlx)?;

    let mut sandboxes_out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut sandbox: Sandbox = row.into();
        if let Ok(vm) = vms::get(env.pool(), sandbox.vm_id).await {
            sandbox.vm_status = Some(vm.status);
        }
        if let Ok(interfaces) = network_interfaces::list_by_vm(env.pool(), sandbox.vm_id).await {
            sandbox.ip_address = interfaces.into_iter().find_map(|i| i.ip_address);
        }
        sandboxes_out.push(sandbox);
    }

    Ok(ApiResponse {
        data: sandboxes_out,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/sandboxes/{sandbox_id}",
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
    let row = sandboxes::get(env.pool(), sandbox_id).await?;
    let mut sandbox: Sandbox = row.into();

    if let Ok(vm) = vms::get(env.pool(), sandbox.vm_id).await {
        sandbox.vm_status = Some(vm.status);
    }
    if let Ok(interfaces) = network_interfaces::list_by_vm(env.pool(), sandbox.vm_id).await {
        sandbox.ip_address = interfaces.into_iter().find_map(|i| i.ip_address);
    }

    // Bump last_activity_at so this GET counts as activity
    let _ = sandboxes::touch_activity(env.pool(), sandbox_id).await;

    Ok(ApiResponse {
        data: sandbox,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    delete,
    path = "/sandboxes/{sandbox_id}",
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

    sandboxes::update_status(env.pool(), sandbox_id, SandboxStatus::Destroying, None)
        .await
        .map_err(crate::errors::Error::Sqlx)?;

    destroy_vm(env.pool(), vm_id).await;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::NO_CONTENT,
    })
}
