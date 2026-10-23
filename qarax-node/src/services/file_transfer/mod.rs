mod cleanup;

use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

type FileLock = tokio::sync::Mutex<()>;
#[derive(Default)]
struct FileLocks(Mutex<HashMap<PathBuf, Weak<FileLock>>>);

impl FileLocks {
    fn get(&self, path: &Path) -> Arc<FileLock> {
        let mut locks = self.0.lock().unwrap_or_else(|e| e.into_inner());
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(FileLock::new(()));
        locks.insert(path.to_owned(), Arc::downgrade(&lock));
        lock
    }
}

use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use crate::overlaybd::manager::OverlayBdManager;
use crate::rpc::node::{
    CopyFileRequest, CreateDiskRequest, DownloadFileRequest, OverlayBdDiskSource, TransferResponse,
    create_disk_request::Source, file_transfer_service_server::FileTransferService,
};

/// Implementation of the FileTransferService gRPC service.
///
/// Handles file downloads (HTTP(S) → disk), local copies, and disk creation on the node.
/// Optionally holds an OverlayBdManager for creating disks from OverlayBD TCMU devices.
#[derive(Clone, Default)]
pub struct FileTransferServiceImpl {
    overlaybd_manager: Option<Arc<OverlayBdManager>>,
    file_locks: Arc<FileLocks>,
}

impl FileTransferServiceImpl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_overlaybd(overlaybd_manager: Option<Arc<OverlayBdManager>>) -> Self {
        Self {
            overlaybd_manager,
            ..Self::default()
        }
    }
}

#[tonic::async_trait]
impl FileTransferService for FileTransferServiceImpl {
    async fn delete_storage_files(
        &self,
        request: Request<crate::rpc::node::DeleteStorageFilesRequest>,
    ) -> Result<Response<crate::rpc::node::DeleteStorageFilesResponse>, Status> {
        let paths = cleanup::paths(request.into_inner()).await?;
        let lock = self.file_locks.get(&paths[0]);
        let _guard = lock.lock().await;
        for path in paths {
            cleanup::unlink(&path).await?;
        }
        Ok(Response::new(
            crate::rpc::node::DeleteStorageFilesResponse {},
        ))
    }

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
                    is_final: true,
                }))
            }
            Err(e) => {
                error!(transfer_id = %req.transfer_id, error = %e, "Download failed");
                Ok(Response::new(TransferResponse {
                    transfer_id: req.transfer_id,
                    success: false,
                    bytes_written: 0,
                    error: e.to_string(),
                    is_final: true,
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
                is_final: true,
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
                    is_final: true,
                }))
            }
            Err(e) => {
                error!(transfer_id = %req.transfer_id, error = %e, "Local copy failed");
                Ok(Response::new(TransferResponse {
                    transfer_id: req.transfer_id,
                    success: false,
                    bytes_written: 0,
                    error: e.to_string(),
                    is_final: true,
                }))
            }
        }
    }

    type CreateDiskStream =
        Pin<Box<dyn futures::Stream<Item = Result<TransferResponse, Status>> + Send + 'static>>;

    async fn create_disk(
        &self,
        request: Request<CreateDiskRequest>,
    ) -> Result<Response<Self::CreateDiskStream>, Status> {
        let req = request.into_inner();
        // Hold the same per-file lock as deletion, including after the caller
        // disconnects while the spawned copy/download is still running.
        let lock = self.file_locks.get(Path::new(&req.path));
        let guard = lock.lock_owned().await;
        info!(
            dest = %req.path,
            size_bytes = req.size_bytes,
            source = ?req.source,
            "Creating disk"
        );

        let (tx, rx) = mpsc::channel(16);
        let overlaybd_manager = self.overlaybd_manager.clone();

        tokio::spawn(async move {
            let _guard = guard;
            let result = match req.source {
                Some(Source::Overlaybd(ref source)) => {
                    do_create_from_overlaybd(
                        overlaybd_manager.as_ref(),
                        &req.path,
                        req.size_bytes,
                        source,
                        &tx,
                    )
                    .await
                }
                Some(Source::Url(ref source)) => do_download(&source.url, &req.path).await,
                Some(Source::Blank(ref source)) => {
                    do_create_blank(&req.path, req.size_bytes, source.preallocate).await
                }
                None => do_create_blank(&req.path, req.size_bytes, false).await,
            };

            let final_msg = match result {
                Ok(bytes_written) => {
                    info!(dest = %req.path, bytes_written, "Disk created");
                    TransferResponse {
                        transfer_id: String::new(),
                        success: true,
                        bytes_written,
                        error: String::new(),
                        is_final: true,
                    }
                }
                Err(e) => {
                    error!(dest = %req.path, error = %e, "Disk creation failed");
                    let _ = tokio::fs::remove_file(&req.path).await;
                    TransferResponse {
                        transfer_id: String::new(),
                        success: false,
                        bytes_written: 0,
                        error: e.to_string(),
                        is_final: true,
                    }
                }
            };
            let _ = tx.send(Ok(final_msg)).await;
        });

        Ok(Response::new(Box::pin(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        )))
    }
}

/// Mount an OverlayBD TCMU device and copy its contents to a raw disk file.
///
/// Uses a temporary mount ID (`commit-{random}`) so it doesn't collide with any
/// existing VM mount.  The device is unmounted after the copy completes.
async fn do_create_from_overlaybd(
    manager: Option<&Arc<OverlayBdManager>>,
    path: &str,
    size_bytes: i64,
    source: &OverlayBdDiskSource,
    progress_tx: &mpsc::Sender<Result<TransferResponse, Status>>,
) -> anyhow::Result<i64> {
    let mgr = manager.ok_or_else(|| {
        anyhow::anyhow!("OverlayBD manager not available — cannot create disk from OCI image")
    })?;

    let mount_id = format!("commit-{}", uuid::Uuid::new_v4());

    // Mount the OverlayBD TCMU device
    let mounted = mgr
        .mount(
            &mount_id,
            &source.image_ref,
            &source.registry_url,
            source.upper_data.as_deref(),
            source.upper_index.as_deref(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("OverlayBD mount failed: {}", e))?;

    let device_path = mounted.device_path.clone();

    // Ensure the target directory exists
    let dest = std::path::Path::new(path);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Copy block device contents to the target file, then unmount regardless of outcome
    let copy_result = copy_block_device(&device_path, path, size_bytes, progress_tx).await;
    mgr.unmount(&mount_id).await;
    copy_result
}

/// Copy exactly `size_bytes` from a block device to a new file.
///
/// Reads in 4 MiB chunks using async I/O.
/// Sends progress updates (is_final=false) through `progress_tx` every 64 MiB.
async fn copy_block_device(
    device_path: &str,
    dest_path: &str,
    size_bytes: i64,
    progress_tx: &mpsc::Sender<Result<TransferResponse, Status>>,
) -> anyhow::Result<i64> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let size = size_bytes as u64;
    info!(
        src = device_path,
        dest = dest_path,
        size_bytes,
        "Copying block device to raw file"
    );

    let mut src = tokio::fs::File::open(device_path).await?;
    let mut dst = tokio::fs::File::create(dest_path).await?;

    const BUF_SIZE: usize = 4 * 1024 * 1024; // 4 MiB
    const PROGRESS_INTERVAL: u64 = 64 * 1024 * 1024; // report every 64 MiB
    let mut buf = vec![0u8; BUF_SIZE];
    let mut bytes_written: u64 = 0;
    let mut last_reported: u64 = 0;

    while bytes_written < size {
        let to_read = ((size - bytes_written) as usize).min(BUF_SIZE);
        let n = src.read(&mut buf[..to_read]).await?;
        if n == 0 {
            return Err(anyhow::anyhow!(
                "block device {} is smaller than requested size_bytes {} (read {} bytes)",
                device_path,
                size_bytes,
                bytes_written
            ));
        }
        dst.write_all(&buf[..n]).await?;
        bytes_written += n as u64;

        if bytes_written - last_reported >= PROGRESS_INTERVAL {
            last_reported = bytes_written;
            debug!(
                bytes_written,
                total_bytes = size,
                "Block device copy progress"
            );
            let _ = progress_tx
                .send(Ok(TransferResponse {
                    transfer_id: String::new(),
                    success: true,
                    bytes_written: bytes_written as i64,
                    error: String::new(),
                    is_final: false,
                }))
                .await;
        }
    }

    dst.flush().await?;
    info!(dest = dest_path, size_bytes, "Block device copy complete");
    Ok(size_bytes)
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

#[cfg(test)]
mod cleanup_tests {
    use super::*;

    #[tokio::test]
    async fn deletion_waits_for_a_writer_before_unlinking() {
        let directory = tempfile::tempdir().unwrap();
        let object_id = uuid::Uuid::new_v4().to_string();
        let path = directory.path().join(&object_id);
        let service = FileTransferServiceImpl::new();
        let writer = service.file_locks.get(&path).lock_owned().await;
        let req = crate::rpc::node::DeleteStorageFilesRequest {
            pool_id: uuid::Uuid::new_v4().to_string(),
            object_id,
            pool_kind: crate::rpc::node::StoragePoolKind::Local as i32,
            config_json: serde_json::json!({"path": directory.path()}).to_string(),
            upper_layer: false,
        };
        let mut deletion =
            tokio::spawn(async move { service.delete_storage_files(Request::new(req)).await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut deletion)
                .await
                .is_err()
        );
        tokio::fs::write(&path, b"last write").await.unwrap();
        drop(writer);
        deletion.await.unwrap().unwrap();
        assert!(!path.exists());
    }
}
