use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::{error, info, instrument};
use uuid::Uuid;

use crate::{
    App,
    model::{
        storage_objects::{self, NewStorageObject},
        storage_pools,
        transfers::{self, NewTransfer, Transfer, TransferType},
    },
    transfer_executor::executor_for_pool,
};

use super::{ApiResponse, Result};

/// Infer the transfer type from the source string.
fn infer_transfer_type(source: &str) -> TransferType {
    if source.starts_with("http://") || source.starts_with("https://") {
        TransferType::Download
    } else {
        TransferType::LocalCopy
    }
}

#[utoipa::path(
    post,
    path = "/storage-pools/{pool_id}/transfers",
    params(
        ("pool_id" = uuid::Uuid, Path, description = "Storage pool unique identifier")
    ),
    request_body = NewTransfer,
    responses(
        (status = 202, description = "Transfer accepted and started", body = Transfer),
        (status = 404, description = "Storage pool not found"),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "transfers"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Path(pool_id): Path<Uuid>,
    Json(new_transfer): Json<NewTransfer>,
) -> Result<ApiResponse<Transfer>> {
    // Validate pool exists
    let pool = storage_pools::get(env.pool(), pool_id).await?;

    let transfer_type = infer_transfer_type(&new_transfer.source);

    // Insert transfer record
    let transfer = transfers::create(env.pool(), pool_id, &new_transfer, transfer_type).await?;

    // Resolve executor for this pool type
    let executor = executor_for_pool(&pool);

    // Spawn background task — HTTP response returns immediately
    let bg_transfer = transfer.clone();
    let bg_pool = pool;
    let db_pool = env.pool_arc();

    tokio::spawn(async move {
        let transfer_id = bg_transfer.id;
        info!(transfer_id = %transfer_id, "Starting background transfer");

        // Mark as running
        if let Err(e) = transfers::mark_running(&db_pool, transfer_id).await {
            error!(transfer_id = %transfer_id, error = %e, "Failed to mark transfer as running");
            return;
        }

        // Execute the transfer
        match executor.execute(&bg_transfer, &bg_pool, &db_pool).await {
            Ok(result) => {
                // Create the storage object
                let new_object = NewStorageObject {
                    name: bg_transfer.name.clone(),
                    storage_pool_id: Some(bg_transfer.storage_pool_id),
                    object_type: bg_transfer.object_type.clone(),
                    size_bytes: result.bytes_written,
                    config: result.storage_config,
                    parent_id: None,
                };

                match storage_objects::create(&db_pool, new_object).await {
                    Ok(object_id) => {
                        if let Err(e) = transfers::mark_completed(
                            &db_pool,
                            transfer_id,
                            object_id,
                            result.bytes_written,
                        )
                        .await
                        {
                            error!(
                                transfer_id = %transfer_id,
                                error = %e,
                                "Failed to mark transfer as completed"
                            );
                        } else {
                            info!(
                                transfer_id = %transfer_id,
                                storage_object_id = %object_id,
                                bytes = result.bytes_written,
                                "Transfer completed"
                            );
                        }
                    }
                    Err(e) => {
                        let msg = format!("Failed to create storage object: {}", e);
                        error!(transfer_id = %transfer_id, error = %msg);
                        let _ = transfers::mark_failed(&db_pool, transfer_id, &msg).await;
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                error!(transfer_id = %transfer_id, error = %msg, "Transfer failed");
                let _ = transfers::mark_failed(&db_pool, transfer_id, &msg).await;
            }
        }
    });

    Ok(ApiResponse {
        data: transfer,
        code: StatusCode::ACCEPTED,
    })
}

#[utoipa::path(
    get,
    path = "/storage-pools/{pool_id}/transfers",
    params(
        ("pool_id" = uuid::Uuid, Path, description = "Storage pool unique identifier"),
        crate::handlers::NameQuery
    ),
    responses(
        (status = 200, description = "List transfers for this pool", body = Vec<Transfer>),
        (status = 500, description = "Internal server error")
    ),
    tag = "transfers"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    Path(pool_id): Path<Uuid>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::NameQuery>,
) -> Result<ApiResponse<Vec<Transfer>>> {
    let transfers = transfers::list_by_pool(env.pool(), pool_id, query.name.as_deref()).await?;
    Ok(ApiResponse {
        data: transfers,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/storage-pools/{pool_id}/transfers/{transfer_id}",
    params(
        ("pool_id" = uuid::Uuid, Path, description = "Storage pool unique identifier"),
        ("transfer_id" = uuid::Uuid, Path, description = "Transfer unique identifier")
    ),
    responses(
        (status = 200, description = "Transfer details", body = Transfer),
        (status = 404, description = "Transfer not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "transfers"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path((_pool_id, transfer_id)): Path<(Uuid, Uuid)>,
) -> Result<ApiResponse<Transfer>> {
    let transfer = transfers::get(env.pool(), transfer_id).await?;
    Ok(ApiResponse {
        data: transfer,
        code: StatusCode::OK,
    })
}
