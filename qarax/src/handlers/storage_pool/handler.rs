use super::*;
use crate::{
    App,
    grpc_client::NodeClient,
    model::{
        hosts,
        jobs::{self, JobType, NewJob},
        storage_objects::{self, NewStorageObject, StorageObjectType},
        storage_pools::{self, NewStoragePool, StoragePool},
    },
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{instrument, warn};
use utoipa::ToSchema;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/storage-pools",
    params(crate::handlers::NameQuery),
    responses(
        (status = 200, description = "List all storage pools", body = Vec<StoragePool>),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::NameQuery>,
) -> Result<ApiResponse<Vec<StoragePool>>> {
    let pools = storage_pools::list(env.pool(), query.name.as_deref()).await?;
    Ok(ApiResponse {
        data: pools,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/storage-pools/{pool_id}",
    params(
        ("pool_id" = uuid::Uuid, Path, description = "Storage pool unique identifier")
    ),
    responses(
        (status = 200, description = "Storage pool found", body = StoragePool),
        (status = 404, description = "Storage pool not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(pool_id): Path<Uuid>,
) -> Result<ApiResponse<StoragePool>> {
    let pool = storage_pools::get(env.pool(), pool_id).await?;
    Ok(ApiResponse {
        data: pool,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/storage-pools",
    request_body = NewStoragePool,
    responses(
        (status = 201, description = "Storage pool created successfully", body = String),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(new_pool): Json<NewStoragePool>,
) -> Result<(StatusCode, String)> {
    let id = storage_pools::create(env.pool(), new_pool.clone()).await?;

    // For shared pool types (NFS, OverlayBD), auto-attach every UP host.
    // Local pools are host-specific and must be attached explicitly.
    if new_pool.pool_type.is_shared() {
        let db_pool = env.pool_arc();
        tokio::spawn(async move {
            let pool = match storage_pools::get(&db_pool, id).await {
                Ok(p) => p,
                Err(e) => {
                    warn!(pool_id = %id, error = %e, "Failed to fetch new pool for host attachment");
                    return;
                }
            };

            let up_hosts = match hosts::list_up(&db_pool).await {
                Ok(h) => h,
                Err(e) => {
                    warn!(pool_id = %id, error = %e, "Failed to list UP hosts for pool attachment");
                    return;
                }
            };

            for host in up_hosts {
                let client = NodeClient::new(&host.address, host.port as u16);
                match client.attach_storage_pool(&pool).await {
                    Ok(()) => {
                        if let Err(e) = storage_pools::attach_host(&db_pool, id, host.id).await {
                            warn!(pool_id = %id, host_id = %host.id, error = %e, "Failed to record pool attachment in DB");
                        }
                    }
                    Err(e) => {
                        warn!(pool_id = %id, host_id = %host.id, error = %e, "Failed to attach storage pool to host via gRPC");
                    }
                }
            }
        });
    }

    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    delete,
    path = "/storage-pools/{pool_id}",
    params(
        ("pool_id" = uuid::Uuid, Path, description = "Storage pool unique identifier")
    ),
    responses(
        (status = 204, description = "Storage pool deleted successfully"),
        (status = 404, description = "Storage pool not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(pool_id): Path<Uuid>,
) -> Result<StatusCode> {
    storage_pools::delete(env.pool(), pool_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AttachPoolHostRequest {
    pub host_id: Uuid,
}

#[utoipa::path(
    post,
    path = "/storage-pools/{pool_id}/hosts",
    params(
        ("pool_id" = uuid::Uuid, Path, description = "Storage pool unique identifier")
    ),
    request_body = AttachPoolHostRequest,
    responses(
        (status = 204, description = "Host attached to storage pool"),
        (status = 404, description = "Storage pool or host not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn attach_host(
    Extension(env): Extension<App>,
    Path(pool_id): Path<Uuid>,
    Json(body): Json<AttachPoolHostRequest>,
) -> Result<StatusCode> {
    let pool = storage_pools::get(env.pool(), pool_id).await?;
    let host = hosts::require_by_id(env.pool(), body.host_id).await?;

    // Perform the real attachment on the node first.
    let client = NodeClient::new(&host.address, host.port as u16);
    client.attach_storage_pool(&pool).await.map_err(|e| {
        tracing::error!(
            pool_id = %pool_id,
            host_id = %body.host_id,
            error = %e,
            "gRPC attach_storage_pool failed"
        );
        crate::errors::Error::UnprocessableEntity(format!("Failed to attach pool to node: {e}"))
    })?;

    // Record the attachment in the DB.
    storage_pools::attach_host(env.pool(), pool_id, body.host_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/storage-pools/{pool_id}/hosts/{host_id}",
    params(
        ("pool_id" = uuid::Uuid, Path, description = "Storage pool unique identifier"),
        ("host_id" = uuid::Uuid, Path, description = "Host unique identifier")
    ),
    responses(
        (status = 204, description = "Host detached from storage pool"),
        (status = 404, description = "Host not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn detach_host(
    Extension(env): Extension<App>,
    Path((pool_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode> {
    let pool = storage_pools::get(env.pool(), pool_id).await?;
    let host = hosts::require_by_id(env.pool(), host_id).await?;

    // Perform the real detachment on the node first.
    let client = NodeClient::new(&host.address, host.port as u16);
    client.detach_storage_pool(&pool).await.map_err(|e| {
        tracing::error!(
            pool_id = %pool_id,
            host_id = %host_id,
            error = %e,
            "gRPC detach_storage_pool failed"
        );
        crate::errors::Error::UnprocessableEntity(format!("Failed to detach pool from node: {e}"))
    })?;

    // Remove the DB record.
    storage_pools::detach_host(env.pool(), pool_id, host_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ImportToPoolRequest {
    /// Human-readable name for the resulting storage object.
    pub name: String,
    /// OCI image reference (e.g. `public.ecr.aws/docker/library/alpine:latest`).
    pub image_ref: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportToPoolResponse {
    pub job_id: Uuid,
    pub storage_object_id: Uuid,
}

/// Import an OCI image into the pool, converting it to OverlayBD format.
#[utoipa::path(
    post,
    path = "/storage-pools/{pool_id}/import",
    params(
        ("pool_id" = Uuid, Path, description = "Storage pool ID")
    ),
    request_body = ImportToPoolRequest,
    responses(
        (status = 202, description = "Import job accepted", body = ImportToPoolResponse),
        (status = 404, description = "Pool not found"),
        (status = 422, description = "No UP host attached to pool"),
        (status = 500, description = "Internal server error")
    ),
    tag = "storage-pools"
)]
#[instrument(skip(env))]
pub async fn import_to_pool(
    Extension(env): Extension<App>,
    Path(pool_id): Path<Uuid>,
    Json(req): Json<ImportToPoolRequest>,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse as _;

    let pool = storage_pools::get(env.pool(), pool_id).await?;

    // Find a host attached to this pool
    let host_id = storage_pools::find_host_for_pool(env.pool(), pool_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find host for pool {}: {}", pool_id, e);
            crate::errors::Error::InternalServerError
        })?
        .ok_or_else(|| {
            crate::errors::Error::UnprocessableEntity(
                "No host attached to this storage pool".into(),
            )
        })?;
    let host = hosts::require_by_id(env.pool(), host_id).await?;

    // Create storage object record
    let storage_object_id = storage_objects::create(
        env.pool(),
        NewStorageObject {
            name: req.name.clone(),
            storage_pool_id: Some(pool_id),
            object_type: StorageObjectType::OciImage,
            size_bytes: 0,
            config: serde_json::json!({ "image_ref": req.image_ref }),
            parent_id: None,
        },
    )
    .await?;

    // Create job record
    let job = jobs::create(
        env.pool(),
        NewJob {
            job_type: JobType::ImagePull,
            description: Some(format!("Importing {} into pool {}", req.image_ref, pool_id)),
            resource_id: Some(storage_object_id),
            resource_type: Some("storage_object".to_string()),
        },
    )
    .await?;
    let job_id = job.id;

    // Spawn background task
    let db_pool = env.pool_arc();
    let image_ref = req.image_ref.clone();
    let pool_config = pool.config.clone();

    tokio::spawn(async move {
        if let Err(e) = jobs::mark_running(&db_pool, job_id).await {
            tracing::error!(job_id = %job_id, error = %e, "Failed to mark import job running");
            return;
        }

        let node_client = NodeClient::new(&host.address, host.port as u16);
        let registry_url =
            match crate::model::storage_pools::OverlayBdPoolConfig::from_value(&pool_config) {
                Some(cfg) => cfg.url,
                None => {
                    let msg = "OverlayBD pool config missing 'url' field".to_string();
                    tracing::error!(pool_id = %pool_id, error = %msg);
                    let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
                    return;
                }
            };

        match node_client
            .import_overlaybd_image(&image_ref, &registry_url)
            .await
        {
            Ok(result) => {
                // Update config with the resolved image_ref from the import
                let config = serde_json::json!({
                    "image_ref": result.image_ref,
                    "digest": result.digest,
                    "registry_url": registry_url,
                });
                let _ = storage_objects::update_config(&db_pool, storage_object_id, &config).await;
                let job_result = serde_json::json!({
                    "image_ref": result.image_ref,
                    "digest": result.digest,
                    "storage_object_id": storage_object_id,
                });
                let _ = jobs::mark_completed(&db_pool, job_id, Some(job_result)).await;
                tracing::info!(pool_id = %pool_id, storage_object_id = %storage_object_id, "Import job completed");
            }
            Err(e) => {
                let msg = format!("Failed to import OverlayBD image: {}", e);
                tracing::error!(pool_id = %pool_id, error = %msg);
                let _ = jobs::mark_failed(&db_pool, job_id, &msg).await;
                // Clean up the storage object on failure
                let _ = storage_objects::delete(&db_pool, storage_object_id).await;
            }
        }
    });

    Ok(ApiResponse {
        data: ImportToPoolResponse {
            job_id,
            storage_object_id,
        },
        code: StatusCode::ACCEPTED,
    }
    .into_response())
}
