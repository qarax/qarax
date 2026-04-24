use async_trait::async_trait;
use sqlx::PgPool;
use tracing::debug;

use crate::grpc_client::NodeClient;
use crate::model::{
    hosts,
    storage_pools::{self, StoragePool, StoragePoolType},
    transfers::{Transfer, TransferType},
};

/// Result of a successful transfer execution.
pub struct TransferResult {
    pub bytes_written: i64,
    /// Config for the resulting storage object (e.g. `{"path": "/var/lib/qarax/images/foo.qcow2"}`)
    pub storage_config: serde_json::Value,
}

/// Error from a transfer execution.
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("no host attached to pool")]
    NoHostAttached,

    #[error("host not found: {0}")]
    HostNotFound(String),

    #[error("pool config missing 'path' field")]
    MissingPoolPath,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("transfer failed: {0}")]
    TransferFailed(String),
}

/// Abstraction for how transfers are executed, decoupled from pool type.
#[async_trait]
pub trait TransferExecutor: Send + Sync {
    async fn execute(
        &self,
        transfer: &Transfer,
        pool: &StoragePool,
        db_pool: &PgPool,
    ) -> Result<TransferResult, TransferError>;
}

/// Executor for LOCAL and NFS storage pools — uses the node gRPC client
/// to download or copy files on any host attached to the pool.
pub struct FilesystemTransferExecutor;

/// Placeholder executor for OverlayBD pools: file-based transfers are not applicable.
/// VM creation with OverlayBD pools uses the dedicated OCI import path instead.
pub struct UnsupportedTransferExecutor;

#[async_trait]
impl TransferExecutor for UnsupportedTransferExecutor {
    async fn execute(
        &self,
        _transfer: &Transfer,
        _pool: &StoragePool,
        _db_pool: &PgPool,
    ) -> Result<TransferResult, TransferError> {
        Err(TransferError::TransferFailed(
            "OverlayBD pools do not support file transfers; use the OCI import path".to_string(),
        ))
    }
}

#[async_trait]
impl TransferExecutor for FilesystemTransferExecutor {
    async fn execute(
        &self,
        transfer: &Transfer,
        pool: &StoragePool,
        db_pool: &PgPool,
    ) -> Result<TransferResult, TransferError> {
        // 1. Find a host attached to this pool
        let host_id = storage_pools::find_host_for_pool(db_pool, pool.id)
            .await?
            .ok_or(TransferError::NoHostAttached)?;
        let host = hosts::get_by_id(db_pool, host_id)
            .await?
            .ok_or_else(|| TransferError::HostNotFound(host_id.to_string()))?;

        // 2. Compute destination path from pool config
        let pool_path = pool
            .config
            .as_object()
            .and_then(|o| o.get("path"))
            .and_then(|v| v.as_str())
            .ok_or(TransferError::MissingPoolPath)?;

        let destination = format!("{}/{}", pool_path, transfer.name);
        debug!(
            transfer_id = %transfer.id,
            destination = %destination,
            "Resolved destination path"
        );

        // 3. Call the node to perform the transfer
        let node_client = NodeClient::new(&host.address, host.port as u16);
        let transfer_id_str = transfer.id.to_string();

        let bytes_written = match transfer.transfer_type {
            TransferType::Download => node_client
                .download_file(&transfer_id_str, &transfer.source, &destination)
                .await
                .map_err(|e| TransferError::TransferFailed(format!("{:#}", e)))?,
            TransferType::LocalCopy => node_client
                .copy_file(&transfer_id_str, &transfer.source, &destination)
                .await
                .map_err(|e| TransferError::TransferFailed(format!("{:#}", e)))?,
        };

        Ok(TransferResult {
            bytes_written,
            storage_config: serde_json::json!({ "path": destination }),
        })
    }
}

/// Resolve the appropriate executor for a given storage pool type.
pub fn executor_for_pool(pool: &StoragePool) -> Box<dyn TransferExecutor> {
    match pool.pool_type {
        StoragePoolType::Local | StoragePoolType::Nfs => Box::new(FilesystemTransferExecutor),
        StoragePoolType::OverlayBd | StoragePoolType::Block => {
            Box::new(UnsupportedTransferExecutor)
        }
    }
}
