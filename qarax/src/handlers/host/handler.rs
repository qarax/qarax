use super::*;
use crate::{
    App,
    grpc_client::NodeClient,
    handlers::audit::{AuditEvent, AuditEventExt},
    handlers::vm::handler::{PlannedVmMigration, execute_planned_vm_migration, plan_vm_migration},
    host_deployer,
    model::{
        audit_log::{AuditAction, AuditResourceType},
        host_gpus::{self, HostGpu},
        host_numa::{self, HostNumaNode},
        hosts::{self, DeployHostRequest, Host, HostStatus, NewHost, UpdateHostRequest},
        jobs::{self, JobType, NewJob},
        network_interfaces, storage_pools,
        vms::{self, Vm, VmStatus},
    },
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

/// Spawns a background task to run a bootc deployment and update host status.
fn spawn_deploy(
    db_pool: Arc<PgPool>,
    host: Host,
    deploy_request: DeployHostRequest,
    host_id: Uuid,
) {
    let image = deploy_request.image.clone();
    tokio::spawn(async move {
        match host_deployer::deploy_bootc_host(&host, &deploy_request).await {
            Ok(_) => {
                info!(host_id = %host_id, "Host deployment finished successfully");
                if let Err(error) = hosts::set_last_deployed_image(&db_pool, host_id, &image).await
                {
                    error!(
                        host_id = %host_id,
                        error = %error,
                        "Failed to store last deployed image"
                    );
                }
                if let Err(error) = hosts::update_status(&db_pool, host_id, HostStatus::Up).await {
                    error!(
                        host_id = %host_id,
                        error = %error,
                        "Failed to mark host status as up after deployment"
                    );
                }
            }
            Err(deploy_error) => {
                error!(
                    host_id = %host_id,
                    error = %deploy_error,
                    "Host deployment failed"
                );
                if let Err(error) =
                    hosts::update_status(&db_pool, host_id, HostStatus::InstallationFailed).await
                {
                    error!(
                        host_id = %host_id,
                        error = %error,
                        "Failed to mark host status as installation_failed"
                    );
                }
            }
        }
    });
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct HostEvacuateResponse {
    pub job_id: Uuid,
}

fn persisted_vm_architecture(vm: &Vm) -> Option<String> {
    vm.config
        .get("architecture")
        .and_then(|value| value.as_str())
        .and_then(common::architecture::normalize_architecture)
}

async fn evacuation_scheduling_request(
    env: &App,
    vm: &Vm,
    source_host_id: Uuid,
) -> Result<hosts::SchedulingRequest> {
    let required_network_ids = network_interfaces::list_by_vm(env.pool(), vm.id)
        .await?
        .into_iter()
        .filter_map(|nic| nic.network_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    Ok(hosts::SchedulingRequest {
        memory_bytes: vm.memory_size,
        vcpus: vm.boot_vcpus,
        disk_bytes: 0,
        architecture: persisted_vm_architecture(vm),
        storage_pool_id: None,
        required_network_ids,
        gpu: None,
        excluded_host_ids: vec![source_host_id],
    })
}

async fn pick_evacuation_plan(
    env: &App,
    vm: &Vm,
    source_host_id: Uuid,
) -> Result<PlannedVmMigration> {
    let base_request = evacuation_scheduling_request(env, vm, source_host_id).await?;
    let mut excluded_host_ids = base_request.excluded_host_ids.clone();
    let mut last_reason = None;

    loop {
        let mut tx = env.pool().begin().await?;
        let candidate = hosts::pick_host_tx(
            &mut tx,
            &hosts::SchedulingRequest {
                excluded_host_ids: excluded_host_ids.clone(),
                ..base_request.clone()
            },
            env.scheduling(),
        )
        .await?;
        drop(tx);

        let Some(candidate) = candidate else {
            let detail = last_reason
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default();
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "no eligible destination host found for VM '{}'{}",
                vm.name, detail
            )));
        };

        match plan_vm_migration(env, vm.id, candidate.id).await {
            Ok(plan) => return Ok(plan),
            Err(crate::errors::Error::UnprocessableEntity(reason)) => {
                excluded_host_ids.push(candidate.id);
                last_reason = Some(reason);
            }
            Err(error) => return Err(error),
        }
    }
}

async fn fail_host_evacuation(
    pool: &PgPool,
    job_id: Uuid,
    host_id: Uuid,
    original_host_status: &HostStatus,
    evacuated_vm_names: &[String],
    error: String,
) {
    let mut error = error;
    let should_restore_host_status = evacuated_vm_names.is_empty();
    let (final_host_status, host_status_restored) = if should_restore_host_status {
        match hosts::update_status(pool, host_id, original_host_status.clone()).await {
            Ok(()) => {
                error.push_str(&format!(
                    "; host status restored to {}",
                    original_host_status
                ));
                (original_host_status.clone(), true)
            }
            Err(update_error) => {
                warn!(
                    host_id = %host_id,
                    error = %update_error,
                    "Failed to restore host status after evacuation failure"
                );
                error.push_str(&format!(
                    "; failed to restore host status to {}: {}",
                    original_host_status, update_error
                ));
                (HostStatus::Maintenance, false)
            }
        }
    } else {
        error.push_str(&format!(
            "; host left in maintenance after evacuating {} VM(s)",
            evacuated_vm_names.len()
        ));
        (HostStatus::Maintenance, false)
    };

    let remaining_vm_names = match vms::list_by_host(pool, host_id).await {
        Ok(vms) => Some(vms.into_iter().map(|vm| vm.name).collect::<Vec<_>>()),
        Err(list_error) => {
            warn!(
                host_id = %host_id,
                error = %list_error,
                "Failed to list remaining resident VMs after evacuation failure"
            );
            None
        }
    };

    let result = serde_json::json!({
        "evacuated_vms": evacuated_vm_names,
        "remaining_vms": remaining_vm_names,
        "host_status": final_host_status,
        "host_status_restored": host_status_restored,
    });

    let _ = jobs::mark_failed_with_result(pool, job_id, &error, Some(result)).await;
}

#[utoipa::path(
    get,
    path = "/hosts",
    params(crate::handlers::HostListQuery),
    responses(
        (status = 200, description = "List all hosts", body = Vec<Host>),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::HostListQuery>,
) -> Result<ApiResponse<Vec<Host>>> {
    let architecture = query
        .architecture
        .as_deref()
        .and_then(common::architecture::normalize_architecture);
    let hosts = hosts::list(env.pool(), query.name.as_deref(), architecture.as_deref()).await?;
    Ok(ApiResponse {
        data: hosts,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/hosts",
    request_body = NewHost,
    responses(
        (status = 201, description = "Host created successfully", body = String),
        (status = 422, description = "Invalid input"),
        (status = 409, description = "Host with name already exists"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn add(
    Extension(env): Extension<App>,
    Json(host): Json<NewHost>,
) -> Result<axum::response::Response> {
    host.validate_unique_name(env.pool()).await?;
    let host_name = host.name.clone();
    let id = hosts::add(env.pool(), &host).await?;

    Ok(
        (StatusCode::CREATED, id.to_string()).with_audit_event(AuditEvent {
            action: AuditAction::Create,
            resource_type: AuditResourceType::Host,
            resource_id: id,
            resource_name: Some(host_name),
            metadata: None,
        }),
    )
}

#[utoipa::path(
    patch,
    path = "/hosts/{host_id}",
    params(
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    request_body = UpdateHostRequest,
    responses(
        (status = 200, description = "Host updated successfully"),
        (status = 404, description = "Host not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn update(
    Extension(env): Extension<App>,
    Path(host_id): Path<Uuid>,
    Json(body): Json<UpdateHostRequest>,
) -> Result<ApiResponse<()>> {
    hosts::update_status(env.pool(), host_id, body.status).await?;
    Ok(ApiResponse {
        data: (),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/hosts/{host_id}/evacuate",
    params(
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    responses(
        (status = 202, description = "Host evacuation accepted", body = HostEvacuateResponse),
        (status = 404, description = "Host not found"),
        (status = 422, description = "Host or resident VMs are not evacuatable")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn evacuate(
    Extension(env): Extension<App>,
    Path(host_id): Path<Uuid>,
) -> Result<axum::response::Response> {
    let host = hosts::require_by_id(env.pool(), host_id).await?;
    let original_host_status = host.status.clone();
    match host.status {
        HostStatus::Up | HostStatus::Maintenance => {}
        _ => {
            return Err(crate::errors::Error::UnprocessableEntity(format!(
                "host {} must be up or maintenance to evacuate (current status: {})",
                host.name, host.status
            )));
        }
    }

    let resident_vms = vms::list_by_host(env.pool(), host_id).await?;
    let unsupported_states: Vec<String> = resident_vms
        .iter()
        .filter(|vm| !matches!(vm.status, VmStatus::Running | VmStatus::Paused))
        .map(|vm| format!("{} ({})", vm.name, vm.status))
        .collect();
    if !unsupported_states.is_empty() {
        return Err(crate::errors::Error::UnprocessableEntity(format!(
            "host evacuation currently supports only running or paused VMs; incompatible residents: {}",
            unsupported_states.join(", ")
        )));
    }

    for vm in &resident_vms {
        pick_evacuation_plan(&env, vm, host_id).await?;
    }

    hosts::update_status(env.pool(), host_id, HostStatus::Maintenance).await?;

    let job = jobs::create(
        env.pool(),
        NewJob {
            job_type: JobType::HostEvacuate,
            description: Some(format!("Evacuating host {} ({})", host.name, host.id)),
            resource_id: Some(host_id),
            resource_type: Some(jobs::resource_types::HOST.to_string()),
        },
    )
    .await?;
    let job_id = job.id;
    let db_pool = env.pool_arc();
    let env_clone = env.clone();
    let resident_vm_ids: Vec<Uuid> = resident_vms.iter().map(|vm| vm.id).collect();
    let original_host_status_clone = original_host_status.clone();

    tokio::spawn(async move {
        if let Err(e) = jobs::mark_running(&db_pool, job_id).await {
            error!(job_id = %job_id, error = %e, "Failed to mark host evacuation job as running");
            fail_host_evacuation(
                db_pool.as_ref(),
                job_id,
                host_id,
                &original_host_status_clone,
                &[],
                format!("failed to start host evacuation job: {e}"),
            )
            .await;
            return;
        }

        let total = resident_vm_ids.len().max(1);
        let mut evacuated_vm_names = Vec::new();
        for (idx, vm_id) in resident_vm_ids.into_iter().enumerate() {
            let vm = match vms::get(&db_pool, vm_id).await {
                Ok(vm) => vm,
                Err(error) => {
                    fail_host_evacuation(
                        db_pool.as_ref(),
                        job_id,
                        host_id,
                        &original_host_status_clone,
                        &evacuated_vm_names,
                        format!("failed to load resident VM {vm_id}: {error}"),
                    )
                    .await;
                    return;
                }
            };

            let plan = match pick_evacuation_plan(&env_clone, &vm, host_id).await {
                Ok(plan) => plan,
                Err(error) => {
                    fail_host_evacuation(
                        db_pool.as_ref(),
                        job_id,
                        host_id,
                        &original_host_status_clone,
                        &evacuated_vm_names,
                        format!("failed to plan evacuation for VM '{}': {}", vm.name, error),
                    )
                    .await;
                    return;
                }
            };

            if let Err(error) = vms::update_status(&db_pool, vm_id, VmStatus::Migrating).await {
                fail_host_evacuation(
                    db_pool.as_ref(),
                    job_id,
                    host_id,
                    &original_host_status_clone,
                    &evacuated_vm_names,
                    format!("failed to mark VM '{}' migrating: {error}", vm.name),
                )
                .await;
                return;
            }

            let original_status = plan.original_status.clone();
            if let Err(msg) = execute_planned_vm_migration(db_pool.as_ref(), plan, None).await {
                let _ = vms::update_status(&db_pool, vm_id, original_status).await;
                fail_host_evacuation(
                    db_pool.as_ref(),
                    job_id,
                    host_id,
                    &original_host_status_clone,
                    &evacuated_vm_names,
                    msg,
                )
                .await;
                return;
            }

            evacuated_vm_names.push(vm.name);
            let progress = (((idx + 1) * 100) / total) as i32;
            let _ = jobs::update_progress(&db_pool, job_id, progress).await;
        }

        let result = serde_json::json!({ "evacuated_vms": resident_vms.len() });
        let _ = jobs::mark_completed(&db_pool, job_id, Some(result)).await;
    });

    use axum::response::IntoResponse as _;
    Ok(ApiResponse {
        data: HostEvacuateResponse { job_id },
        code: StatusCode::ACCEPTED,
    }
    .with_audit_event(AuditEvent {
        action: AuditAction::Evacuate,
        resource_type: AuditResourceType::Host,
        resource_id: host_id,
        resource_name: Some(host.name),
        metadata: None,
    })
    .into_response())
}

#[utoipa::path(
    post,
    path = "/hosts/{host_id}/deploy",
    params(
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    request_body = DeployHostRequest,
    responses(
        (status = 202, description = "Host deployment accepted", body = String),
        (status = 404, description = "Host not found"),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn deploy(
    Extension(env): Extension<App>,
    Path(host_id): Path<Uuid>,
    Json(body): Json<DeployHostRequest>,
) -> Result<axum::response::Response> {
    body.validate()?;

    let host = hosts::require_by_id(env.pool(), host_id).await?;
    let host_name = host.name.clone();
    hosts::update_status(env.pool(), host_id, HostStatus::Installing).await?;

    spawn_deploy(env.pool_arc(), host, body, host_id);

    Ok(
        (StatusCode::ACCEPTED, "Host deployment started".to_string()).with_audit_event(
            AuditEvent {
                action: AuditAction::Deploy,
                resource_type: AuditResourceType::Host,
                resource_id: host_id,
                resource_name: Some(host_name),
                metadata: None,
            },
        ),
    )
}

#[utoipa::path(
    post,
    path = "/hosts/{host_id}/init",
    params(
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    responses(
        (status = 200, description = "Host initialized successfully", body = Host),
        (status = 404, description = "Host not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn init(
    Extension(env): Extension<App>,
    Path(host_id): Path<Uuid>,
) -> Result<ApiResponse<Host>> {
    let host = hosts::require_by_id(env.pool(), host_id).await?;

    let node_client = crate::grpc_client::NodeClient::new(&host.address, host.port as u16);

    let node_info = node_client.get_node_info().await.map_err(|e| {
        tracing::error!("Failed to get node info for host {}: {}", host_id, e);
        crate::errors::Error::InternalServerError
    })?;

    info!(
        host_id = %host_id,
        hostname = %node_info.hostname,
        ch_version = %node_info.cloud_hypervisor_version,
        fc_version = node_info.firecracker_version.as_deref().unwrap_or("-"),
        kernel_version = %node_info.kernel_version,
        total_cpus = node_info.total_cpus,
        total_memory_bytes = node_info.total_memory_bytes,
        load_average = node_info.load_average_1m,
        "Node info retrieved"
    );

    hosts::update_versions(
        env.pool(),
        host_id,
        &node_info.cloud_hypervisor_version,
        node_info.firecracker_version.as_deref(),
        &node_info.kernel_version,
        &node_info.node_version,
    )
    .await?;

    hosts::update_resources(
        env.pool(),
        host_id,
        Some(&node_info.architecture),
        node_info.total_cpus,
        node_info.total_memory_bytes,
        node_info.available_memory_bytes,
        node_info.load_average_1m,
        node_info.disk_total_bytes,
        node_info.disk_available_bytes,
    )
    .await?;

    hosts::update_status(env.pool(), host_id, HostStatus::Up).await?;
    let numa_discoveries: Vec<host_numa::NumaNodeDiscovery> = node_info
        .numa_nodes
        .iter()
        .map(|n| host_numa::NumaNodeDiscovery {
            node_id: n.id,
            cpu_list: n
                .cpus
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(","),
            memory_bytes: if n.memory_bytes > 0 {
                Some(n.memory_bytes)
            } else {
                None
            },
            distances: n.distances.clone(),
        })
        .collect();

    if let Err(e) = host_numa::sync_numa_nodes(env.pool(), host_id, &numa_discoveries).await {
        warn!(host_id = %host_id, error = %e, "Failed to sync NUMA nodes");
    }

    let updated_host = hosts::require_by_id(env.pool(), host_id).await?;

    // Background: re-attach pools this host already belongs to (including local pools),
    // and attach any shared pools (NFS, OverlayBD) that it hasn't joined yet.
    let db_pool = env.pool_arc();
    let host_address = host.address.clone();
    let host_port = host.port;
    tokio::spawn(async move {
        let existing_pools = match storage_pools::list_for_host(&db_pool, host_id).await {
            Ok(p) => p,
            Err(e) => {
                warn!(host_id = %host_id, error = %e, "Failed to list existing pools for host");
                return;
            }
        };

        let all_pools = match storage_pools::list(&db_pool, None).await {
            Ok(p) => p,
            Err(e) => {
                warn!(host_id = %host_id, error = %e, "Failed to list storage pools for host init");
                return;
            }
        };

        let existing_ids: std::collections::HashSet<Uuid> =
            existing_pools.iter().map(|p| p.id).collect();

        // Shared pools this host hasn't joined yet
        let new_shared: Vec<_> = all_pools
            .into_iter()
            .filter(|p| p.pool_type.is_shared() && !existing_ids.contains(&p.id))
            .collect();

        let client = NodeClient::new(&host_address, host_port as u16);

        // Re-attach existing pools via gRPC (e.g. recreate local dirs after reboot)
        for pool in &existing_pools {
            if let Err(e) = client.attach_storage_pool(pool).await {
                warn!(
                    host_id = %host_id,
                    pool_id = %pool.id,
                    error = %e,
                    "Failed to re-attach storage pool to host via gRPC"
                );
            }
        }

        // Attach new shared pools
        for pool in &new_shared {
            match client.attach_storage_pool(pool).await {
                Ok(()) => {
                    if let Err(e) = storage_pools::attach_host(&db_pool, pool.id, host_id).await {
                        warn!(
                            host_id = %host_id,
                            pool_id = %pool.id,
                            error = %e,
                            "Failed to record pool attachment in DB"
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        host_id = %host_id,
                        pool_id = %pool.id,
                        error = %e,
                        "Failed to attach shared storage pool to host via gRPC"
                    );
                }
            }
        }
    });

    Ok(ApiResponse {
        data: updated_host,
        code: StatusCode::OK,
    })
}

#[cfg(test)]
mod tests {
    use sqlx::{Connection, Executor, PgConnection, PgPool};
    use uuid::Uuid;

    use super::fail_host_evacuation;
    use crate::{
        configuration::get_configuration,
        model::{
            hosts::{self, HostStatus, NewHost},
            jobs::{self, JobStatus, JobType, NewJob},
        },
    };

    struct TestDatabase {
        name: String,
        pool: PgPool,
    }

    impl TestDatabase {
        async fn new() -> Self {
            let mut configuration = get_configuration().expect("Failed to read configuration");
            configuration.database.name = Uuid::new_v4().to_string();

            let mut connection =
                PgConnection::connect(&configuration.database.connection_string_without_db())
                    .await
                    .expect("Failed to connect to Postgres");
            connection
                .execute(format!(r#"CREATE DATABASE "{}";"#, configuration.database.name).as_str())
                .await
                .expect("Failed to create test database");

            let pool = PgPool::connect(&configuration.database.connection_string())
                .await
                .expect("Failed to connect to test database");
            sqlx::migrate!("../migrations")
                .run(&pool)
                .await
                .expect("Failed to run migrations");

            Self {
                name: configuration.database.name,
                pool,
            }
        }

        async fn insert_host(&self, name: &str, status: HostStatus) -> Uuid {
            let host_id = hosts::add(
                &self.pool,
                &NewHost {
                    name: name.to_string(),
                    address: "127.0.0.1".to_string(),
                    port: 50051,
                    host_user: "root".to_string(),
                    password: String::new(),
                },
            )
            .await
            .expect("Failed to insert host");
            hosts::update_status(&self.pool, host_id, status)
                .await
                .expect("Failed to update host status");
            host_id
        }

        async fn insert_host_evacuation_job(&self, host_id: Uuid) -> Uuid {
            jobs::create(
                &self.pool,
                NewJob {
                    job_type: JobType::HostEvacuate,
                    description: Some("test evacuation".to_string()),
                    resource_id: Some(host_id),
                    resource_type: Some(jobs::resource_types::HOST.to_string()),
                },
            )
            .await
            .expect("Failed to create job")
            .id
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let db_name = self.name.clone();
            let (tx, rx) = std::sync::mpsc::channel();

            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                rt.block_on(async move {
                    let configuration = get_configuration().expect("Failed to read configuration");
                    let mut connection =
                        PgConnection::connect_with(&configuration.database.without_db())
                            .await
                            .expect("Failed to connect to Postgres");
                    connection
                        .execute(&*format!(r#"DROP DATABASE "{}" WITH (FORCE)"#, db_name))
                        .await
                        .expect("Failed to drop test database");
                    let _ = tx.send(());
                });
            });

            let _ = rx.recv();
        }
    }

    #[tokio::test]
    async fn fail_host_evacuation_restores_original_status_when_no_vm_moved() {
        let db = TestDatabase::new().await;
        let host_id = db.insert_host("host-no-progress", HostStatus::Up).await;
        hosts::update_status(&db.pool, host_id, HostStatus::Maintenance)
            .await
            .expect("Failed to mark host maintenance");
        let job_id = db.insert_host_evacuation_job(host_id).await;

        fail_host_evacuation(
            &db.pool,
            job_id,
            host_id,
            &HostStatus::Up,
            &[],
            "boom".to_string(),
        )
        .await;

        let host = hosts::require_by_id(&db.pool, host_id)
            .await
            .expect("Failed to reload host");
        assert_eq!(host.status, HostStatus::Up);

        let job = jobs::get(&db.pool, job_id)
            .await
            .expect("Failed to reload job");
        assert_eq!(job.status, JobStatus::Failed);
        let result = job.result.expect("Expected failure result metadata");
        assert_eq!(result["host_status"], serde_json::json!("up"));
        assert_eq!(result["host_status_restored"], serde_json::json!(true));
        assert_eq!(result["evacuated_vms"], serde_json::json!([]));
        assert!(
            job.error
                .expect("Expected job error")
                .contains("host status restored to up")
        );
    }

    #[tokio::test]
    async fn fail_host_evacuation_keeps_maintenance_after_partial_progress() {
        let db = TestDatabase::new().await;
        let host_id = db
            .insert_host("host-partial-progress", HostStatus::Maintenance)
            .await;
        let job_id = db.insert_host_evacuation_job(host_id).await;

        fail_host_evacuation(
            &db.pool,
            job_id,
            host_id,
            &HostStatus::Up,
            &[String::from("vm-a")],
            "boom".to_string(),
        )
        .await;

        let host = hosts::require_by_id(&db.pool, host_id)
            .await
            .expect("Failed to reload host");
        assert_eq!(host.status, HostStatus::Maintenance);

        let job = jobs::get(&db.pool, job_id)
            .await
            .expect("Failed to reload job");
        assert_eq!(job.status, JobStatus::Failed);
        let result = job.result.expect("Expected failure result metadata");
        assert_eq!(result["host_status"], serde_json::json!("maintenance"));
        assert_eq!(result["host_status_restored"], serde_json::json!(false));
        assert_eq!(result["evacuated_vms"], serde_json::json!(["vm-a"]));
        assert!(
            job.error
                .expect("Expected job error")
                .contains("host left in maintenance after evacuating 1 VM(s)")
        );
    }
}

#[utoipa::path(
    post,
    path = "/hosts/{host_id}/upgrade",
    params(
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    responses(
        (status = 202, description = "Node upgrade accepted", body = String),
        (status = 400, description = "No deployed image recorded for this host"),
        (status = 404, description = "Host not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn node_upgrade(
    Extension(env): Extension<App>,
    Path(host_id): Path<Uuid>,
) -> Result<(StatusCode, String)> {
    let host = hosts::require_by_id(env.pool(), host_id).await?;

    let image = host.last_deployed_image.clone().ok_or_else(|| {
        crate::errors::Error::UnprocessableEntity(
            "no deployed image recorded for this host; run /deploy first".to_string(),
        )
    })?;

    let password = String::from_utf8(host.password.clone()).unwrap_or_default();
    let deploy_request = DeployHostRequest {
        image,
        ssh_port: None,
        ssh_user: Some(host.host_user.clone()),
        ssh_password: if password.is_empty() {
            None
        } else {
            Some(password)
        },
        ssh_private_key_path: None,
        install_bootc: Some(false),
        reboot: Some(true),
    };

    hosts::update_status(env.pool(), host_id, HostStatus::Installing).await?;

    spawn_deploy(env.pool_arc(), host, deploy_request, host_id);

    Ok((StatusCode::ACCEPTED, "Node upgrade started".to_string()))
}

#[utoipa::path(
    get,
    path = "/hosts/{host_id}/gpus",
    params(
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    responses(
        (status = 200, description = "List GPUs on the host", body = Vec<HostGpu>),
        (status = 404, description = "Host not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn list_gpus(
    Extension(env): Extension<App>,
    Path(host_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<HostGpu>>> {
    // Verify host exists (returns 404 for unknown host_id)
    hosts::require_by_id(env.pool(), host_id).await?;
    let gpus = host_gpus::list_by_host(env.pool(), host_id).await?;
    Ok(ApiResponse {
        data: gpus,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/hosts/{host_id}/numa",
    params(
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    responses(
        (status = 200, description = "List NUMA nodes discovered on the host", body = Vec<HostNumaNode>),
        (status = 404, description = "Host not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn list_numa_nodes(
    Extension(env): Extension<App>,
    Path(host_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<HostNumaNode>>> {
    hosts::require_by_id(env.pool(), host_id).await?;
    let nodes = host_numa::list_by_host(env.pool(), host_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;
    Ok(ApiResponse {
        data: nodes,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/hosts/{host_id}/resources",
    params(
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    responses(
        (status = 200, description = "Computed host resource ledger", body = crate::model::hosts::HostResourceCapacity),
        (status = 404, description = "Host not found")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn resources(
    Extension(env): Extension<App>,
    Path(host_id): Path<Uuid>,
) -> Result<ApiResponse<crate::model::hosts::HostResourceCapacity>> {
    hosts::require_by_id(env.pool(), host_id).await?;
    let resources = hosts::get_resource_capacity(env.pool(), host_id)
        .await?
        .ok_or(crate::errors::Error::NotFound)?;
    Ok(ApiResponse {
        data: resources,
        code: StatusCode::OK,
    })
}
