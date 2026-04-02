use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use crate::rpc::node::{
    CopyFileRequest, CreateDiskRequest, DownloadFileRequest, TransferResponse,
    file_transfer_service_server::FileTransferService,
};

/// Implementation of the FileTransferService gRPC service.
///
/// Handles file downloads (HTTP(S) → disk), local copies, and disk creation on the node.
#[derive(Clone, Default)]
pub struct FileTransferServiceImpl;

impl FileTransferServiceImpl {
    pub fn new() -> Self {
        Self
    }
}

#[tonic::async_trait]
impl FileTransferService for FileTransferServiceImpl {
    async fn download_file(
        &self,
        request: Request<DownloadFileRequest>,
    ) -> Result<Response<TransferResponse>, Status> {
        let req = request.into_inner();
        info!(
            transfer_id = %req.transfer_id,
            source = %req.source_url,
            dest = %req.destination_path,
            "Starting file download"
        );

        match do_download(&req.source_url, &req.destination_path).await {
            Ok(bytes_written) => {
                info!(transfer_id = %req.transfer_id, bytes_written, "Download completed");
                Ok(Response::new(TransferResponse {
                    transfer_id: req.transfer_id,
                    success: true,
                    bytes_written,
                    error: String::new(),
                }))
            }
            Err(e) => {
                error!(transfer_id = %req.transfer_id, error = %e, "Download failed");
                Ok(Response::new(TransferResponse {
                    transfer_id: req.transfer_id,
                    success: false,
                    bytes_written: 0,
                    error: e.to_string(),
                }))
            }
        }
    }

    async fn copy_file(
        &self,
        request: Request<CopyFileRequest>,
    ) -> Result<Response<TransferResponse>, Status> {
        let req = request.into_inner();
        info!(
            transfer_id = %req.transfer_id,
            source = %req.source_path,
            dest = %req.destination_path,
            "Starting local file copy"
        );

        let dest = std::path::Path::new(&req.destination_path);
        if let Some(parent) = dest.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            error!(error = %e, "Failed to create destination directory");
            return Ok(Response::new(TransferResponse {
                transfer_id: req.transfer_id,
                success: false,
                bytes_written: 0,
                error: format!("Failed to create directory: {e}"),
            }));
        }

        match tokio::fs::copy(&req.source_path, &req.destination_path).await {
            Ok(bytes_written) => {
                info!(transfer_id = %req.transfer_id, bytes_written, "Local copy completed");
                Ok(Response::new(TransferResponse {
                    transfer_id: req.transfer_id,
                    success: true,
                    bytes_written: bytes_written as i64,
                    error: String::new(),
                }))
            }
            Err(e) => {
                error!(transfer_id = %req.transfer_id, error = %e, "Local copy failed");
                Ok(Response::new(TransferResponse {
                    transfer_id: req.transfer_id,
                    success: false,
                    bytes_written: 0,
                    error: e.to_string(),
                }))
            }
        }
    }

    async fn create_disk(
        &self,
        request: Request<CreateDiskRequest>,
    ) -> Result<Response<TransferResponse>, Status> {
        let req = request.into_inner();
        info!(
            dest = %req.path,
            size_bytes = req.size_bytes,
            source_url = ?req.source_url,
            preallocate = req.preallocate.unwrap_or(false),
            "Creating disk"
        );

        let result = if let Some(ref url) = req.source_url {
            do_download(url, &req.path).await
        } else {
            do_create_blank(&req.path, req.size_bytes, req.preallocate.unwrap_or(false)).await
        };

        match result {
            Ok(bytes_written) => {
                info!(dest = %req.path, bytes_written, "Disk created");
                Ok(Response::new(TransferResponse {
                    transfer_id: String::new(),
                    success: true,
                    bytes_written,
                    error: String::new(),
                }))
            }
            Err(e) => {
                error!(dest = %req.path, error = %e, "Disk creation failed");
                let _ = tokio::fs::remove_file(&req.path).await;
                Ok(Response::new(TransferResponse {
                    transfer_id: String::new(),
                    success: false,
                    bytes_written: 0,
                    error: e.to_string(),
                }))
            }
        }
    }
}

/// Create a blank disk file at `path` with logical size `size_bytes`.
///
/// If `preallocate` is true, attempts `fallocate(2)` to reserve blocks upfront.
/// Falls back to sparse (`set_len`) if fallocate is unavailable on the filesystem.
async fn do_create_blank(path: &str, size_bytes: i64, preallocate: bool) -> anyhow::Result<i64> {
    let dest = std::path::Path::new(path);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let file = tokio::fs::File::create(dest).await?;
    let std_file = file.into_std().await;
    let path_owned = path.to_string();

    tokio::task::spawn_blocking(move || -> anyhow::Result<i64> {
        if preallocate {
            use nix::fcntl::{FallocateFlags, fallocate};
            use std::os::unix::io::AsRawFd;
            match fallocate(std_file.as_raw_fd(), FallocateFlags::empty(), 0, size_bytes) {
                Ok(()) => return Ok(size_bytes),
                Err(e) => warn!(path = %path_owned, error = %e, "fallocate failed, falling back to sparse"),
            }
        }
        std_file.set_len(size_bytes as u64)?;
        Ok(size_bytes)
    })
    .await?
}

/// Download a file from `source_url` to `destination_path`, streaming chunks to disk.
///
/// Writes to a `.tmp` sibling file and atomically renames on success.
/// On failure the partial `.tmp` file is cleaned up.
async fn do_download(source_url: &str, destination_path: &str) -> anyhow::Result<i64> {
    let dest = std::path::Path::new(destination_path);
    let tmp_path = dest.with_extension("tmp");

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let response = reqwest::get(source_url).await?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("HTTP {status} from {source_url}");
    }

    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&tmp_path).await?;
    let mut bytes_written: i64 = 0;

    let result: anyhow::Result<i64> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            bytes_written += chunk.len() as i64;
        }
        file.flush().await?;
        Ok(bytes_written)
    }
    .await;

    match result {
        Ok(n) => {
            tokio::fs::rename(&tmp_path, dest).await?;
            debug!(bytes = n, "Download written to {destination_path}");
            Ok(n)
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            Err(e)
        }
    }
}
