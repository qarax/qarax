use super::*;
use crate::{
    App,
    grpc_client::NodeClient,
    host_deployer,
    model::{
        host_gpus::{self, HostGpu},
        hosts::{self, DeployHostRequest, Host, HostStatus, NewHost, UpdateHostRequest},
        storage_pools,
    },
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/hosts",
    params(crate::handlers::NameQuery),
    responses(
        (status = 200, description = "List all hosts", body = Vec<Host>),
        (status = 500, description = "Internal server error")
    ),
    tag = "hosts"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::NameQuery>,
) -> Result<ApiResponse<Vec<Host>>> {
    let hosts = hosts::list(env.pool(), query.name.as_deref()).await?;
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
) -> Result<(StatusCode, String)> {
    host.validate_unique_name(env.pool()).await?;
    let id = hosts::add(env.pool(), &host).await?;
    Ok((StatusCode::CREATED, id.to_string()))
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
) -> Result<(StatusCode, String)> {
    body.validate()?;

    let host = hosts::require_by_id(env.pool(), host_id).await?;
    hosts::update_status(env.pool(), host_id, HostStatus::Installing).await?;

    let db_pool = env.pool_arc();
    tokio::spawn(async move {
        match host_deployer::deploy_bootc_host(&host, &body).await {
            Ok(_) => {
                info!(host_id = %host_id, "Host deployment finished successfully");
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

    Ok((StatusCode::ACCEPTED, "Host deployment started".to_string()))
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
        &node_info.kernel_version,
    )
    .await?;

    hosts::update_resources(
        env.pool(),
        host_id,
        node_info.total_cpus,
        node_info.total_memory_bytes,
        node_info.available_memory_bytes,
        node_info.load_average_1m,
        node_info.disk_total_bytes,
        node_info.disk_available_bytes,
    )
    .await?;

    hosts::update_status(env.pool(), host_id, HostStatus::Up).await?;

    let updated_host = hosts::require_by_id(env.pool(), host_id).await?;

    // Background: attach this host to every existing storage pool via gRPC, then record in DB.
    let db_pool = env.pool_arc();
    let host_address = host.address.clone();
    let host_port = host.port;
    tokio::spawn(async move {
        let pools = match storage_pools::list(&db_pool, None).await {
            Ok(p) => p,
            Err(e) => {
                warn!(host_id = %host_id, error = %e, "Failed to list storage pools for host init");
                return;
            }
        };

        let client = NodeClient::new(&host_address, host_port as u16);
        for pool in pools {
            let pool_id = pool.id;
            match client.attach_storage_pool(&pool).await {
                Ok(()) => {
                    if let Err(e) = storage_pools::attach_host(&db_pool, pool_id, host_id).await {
                        warn!(
                            host_id = %host_id,
                            pool_id = %pool_id,
                            error = %e,
                            "Failed to record pool attachment in DB"
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        host_id = %host_id,
                        pool_id = %pool_id,
                        error = %e,
                        "Failed to attach storage pool to host via gRPC"
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
