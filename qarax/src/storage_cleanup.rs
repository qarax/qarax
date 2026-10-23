//! Durable cleanup of files provisioned by Qarax. Allocation is released by
//! the database only after the node confirms deletion.
use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    App,
    grpc_client::NodeClient,
    model::{hosts, storage_pools},
};

#[derive(sqlx::FromRow)]
struct Cleanup {
    pool_id: Uuid,
    upper_layer: bool,
    host_ids: Vec<Uuid>,
}

/// Returns false while cleanup is pending. Concurrent workers serialize on
/// the queue row; failed RPCs leave a durable record for the next attempt.
pub async fn process(pool: &PgPool, object_id: Uuid) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let task = sqlx::query_as::<_, Cleanup>(
        "SELECT pool_id, upper_layer, host_ids FROM storage_cleanup
         WHERE object_id = $1 FOR UPDATE",
    )
    .bind(object_id)
    .fetch_optional(tx.as_mut())
    .await?;
    let Some(task) = task else {
        return Ok(true);
    };
    let storage_pool = storage_pools::get(pool, task.pool_id).await?;
    let shared = storage_pool.pool_type == storage_pools::StoragePoolType::Nfs;
    let mut remaining = task.host_ids;
    if shared || remaining.is_empty() {
        remaining = storage_pools::list_host_ids(pool, task.pool_id).await?;
    }
    let mut last_error = "No node is available to clean up this pool".to_string();
    let mut completed = false;
    for host_id in remaining.clone() {
        let Some(host) = hosts::get_by_id(pool, host_id).await? else {
            last_error = format!("Cleanup host {host_id} no longer exists");
            continue;
        };
        let client = NodeClient::new(&host.address, host.port as u16);
        match tokio::time::timeout(
            Duration::from_secs(5),
            client.delete_storage_files(&storage_pool, object_id, task.upper_layer),
        )
        .await
        {
            Ok(Ok(())) => {
                remaining.retain(|id| *id != host_id);
                if shared || remaining.is_empty() {
                    completed = true;
                    break;
                }
            }
            Ok(Err(err)) => last_error = err.to_string(),
            Err(_) => last_error = "Node cleanup timed out".to_string(),
        }
    }
    if completed {
        sqlx::query("DELETE FROM storage_cleanup WHERE object_id = $1")
            .bind(object_id)
            .execute(tx.as_mut())
            .await?;
    } else {
        tracing::warn!(%object_id, error = %last_error, "Storage cleanup will be retried");
        sqlx::query(
            "UPDATE storage_cleanup SET host_ids = $2, attempts = attempts + 1,
                     last_error = $3, next_attempt_at = NOW() + INTERVAL '30 seconds'
                     WHERE object_id = $1",
        )
        .bind(object_id)
        .bind(remaining)
        .bind(last_error)
        .execute(tx.as_mut())
        .await?;
    }
    tx.commit().await?;
    Ok(completed)
}

pub async fn start(app: App) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let result = sqlx::query_scalar::<_, Uuid>(
            "SELECT object_id FROM storage_cleanup WHERE next_attempt_at <= NOW()
             ORDER BY next_attempt_at LIMIT 100",
        )
        .fetch_all(app.pool())
        .await;
        match result {
            Ok(ids) => {
                for id in ids {
                    if let Err(err) = process(app.pool(), id).await {
                        tracing::warn!(object_id = %id, error = %err, "Storage cleanup failed");
                    }
                }
            }
            Err(err) => tracing::warn!(error = %err, "Failed to read storage cleanup queue"),
        }
    }
}
