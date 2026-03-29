use axum::{Extension, Json, extract::Path, response::IntoResponse};
use http::StatusCode;
use tokio::time::{Duration, interval};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    App,
    handlers::{ApiResponse, Result},
    model::{
        network_interfaces,
        sandboxes::{self, CreateSandboxResponse, NewSandbox, Sandbox, SandboxStatus},
        vms::{self, NewVm, VmStatus},
    },
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

    // Build a NewVm from the sandbox request — template provides all defaults
    let new_vm = NewVm {
        name: req.name.clone(),
        tags: None,
        vm_template_id: Some(req.vm_template_id),
        instance_type_id: req.instance_type_id,
        hypervisor: None,
        boot_vcpus: None,
        max_vcpus: None,
        cpu_topology: None,
        kvm_hyperv: None,
        memory_size: None,
        memory_hotplug_size: None,
        memory_mergeable: None,
        memory_shared: None,
        memory_hugepages: None,
        memory_hugepage_size: None,
        memory_prefault: None,
        memory_thp: None,
        boot_source_id: None,
        root_disk_object_id: None,
        boot_mode: None,
        description: None,
        image_ref: None,
        cloud_init_user_data: None,
        cloud_init_meta_data: None,
        cloud_init_network_config: None,
        network_id: req.network_id,
        networks: None,
        accelerator_config: None,
        numa_config: None,
        config: serde_json::json!({}),
    };

    let resolved_vm = vms::resolve_create_request(env.pool(), new_vm).await?;

    if resolved_vm.image_ref.is_some() {
        return Err(crate::errors::Error::UnprocessableEntity(
            "sandbox VM templates with OCI image_ref are not supported yet".into(),
        ));
    }

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
        destroy_sandbox_vm(&env, vm_id).await;
        return Err(crate::errors::Error::Sqlx(e));
    }

    // Kick off async VM start
    let job_id = match start_vm_internal(&env, vm_id).await {
        Ok(job_id) => job_id,
        Err(e) => {
            destroy_sandbox_vm(&env, vm_id).await;
            let _ = sandboxes::delete(env.pool(), sandbox_id).await;
            return Err(e);
        }
    };

    // Spawn a watcher that transitions the sandbox to READY/ERROR once the VM settles
    let db_pool = env.pool_arc();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(2));
        let mut attempts = 0u32;
        loop {
            ticker.tick().await;
            attempts += 1;
            // 5-minute timeout
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
                    _ => continue,
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

    destroy_sandbox_vm(&env, vm_id).await;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::NO_CONTENT,
    })
}

/// Stop and delete the VM backing a sandbox. Best-effort: errors are logged but not returned.
pub(crate) async fn destroy_sandbox_vm(env: &App, vm_id: Uuid) {
    use crate::grpc_client::NodeClient;
    use crate::model::{host_gpus, hosts};

    if let Err(e) = host_gpus::deallocate_by_vm(env.pool(), vm_id).await {
        tracing::warn!(vm_id = %vm_id, error = %e, "Failed to deallocate GPUs for sandbox VM");
    }

    let vm = match vms::get(env.pool(), vm_id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(vm_id = %vm_id, error = %e, "Failed to get sandbox VM for deletion");
            let _ = vms::delete(env.pool(), vm_id).await;
            return;
        }
    };

    if let Some(host_id) = vm.host_id
        && let Ok(Some(host)) = hosts::get_by_id(env.pool(), host_id).await
    {
        let client = NodeClient::new(&host.address, host.port as u16);
        if let Err(e) = client.delete_vm(vm_id).await {
            let not_found = e
                .downcast_ref::<crate::errors::Error>()
                .map(|err| matches!(err, crate::errors::Error::NotFound))
                .unwrap_or(false);
            if !not_found {
                tracing::warn!(vm_id = %vm_id, error = %e, "delete_vm on node failed (ignoring)");
            }
        }
    }

    if let Err(e) = vms::delete(env.pool(), vm_id).await {
        tracing::error!(vm_id = %vm_id, error = %e, "Failed to delete sandbox VM from DB");
    }
}
