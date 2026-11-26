//! Snapshots copied between nodes over NBD, such as a VM backup copied to
//! another host. The node that has the snapshot offers its files on its NBD
//! server, the destination node pulls them with nbdwire's client, and the
//! first node withdraws them. Both ends use TLS with the node CA
//! (`scripts/gen-node-tls.sh`): the server presents the node's certificate and
//! wants a client certificate from the CA, and the puller presents its node's.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use nbdwire::tokio_rustls::rustls::pki_types::ServerName;
use nbdwire::tokio_rustls::{TlsAcceptor, TlsConnector};
use nbdwire::{Access, Client, FileBackend, Server};
use tokio::net::{TcpListener, TcpStream};
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

use crate::rpc::node::snapshot_copy_service_server::SnapshotCopyService;
use crate::rpc::node::{
    ExportSnapshotRequest, ExportSnapshotResponse, PullSnapshotRequest, PullSnapshotResponse,
    UnexportSnapshotRequest,
};
use crate::snapshot;

/// NBD's port.
pub const DEFAULT_NBD_PORT: u16 = 10809;

/// Why a copy's request failed, as its gRPC status says.
#[derive(Debug, thiserror::Error)]
enum CopyError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Exists(String),
    /// The node lacks what the copy needs, such as its client certificate.
    #[error("{0}")]
    Precondition(String),
    #[error("{0}")]
    Failed(String),
}

impl From<CopyError> for Status {
    fn from(e: CopyError) -> Self {
        match e {
            CopyError::Invalid(m) => Status::invalid_argument(m),
            CopyError::Exists(m) => Status::already_exists(m),
            CopyError::Precondition(m) => Status::failed_precondition(m),
            CopyError::Failed(m) => Status::internal(m),
        }
    }
}

pub struct SnapshotCopyServiceImpl {
    server: Arc<Server>,
    port: u16,
    /// The node's TLS files, its client certificate among them.
    tls_dir: PathBuf,
    /// The exports of each snapshot on offer, by export ID.
    offered: Mutex<HashMap<String, Vec<String>>>,
}

impl SnapshotCopyServiceImpl {
    /// Serve NBD on `port` (any free one for 0) with the TLS files in
    /// `tls_dir`, offering nothing until a snapshot is exported.
    pub async fn serve(tls_dir: &Path, port: u16) -> anyhow::Result<Self> {
        let tls = crate::tls::nbd_server_config(tls_dir)?;
        let server = Arc::new(Server::with_tls(TlsAcceptor::from(Arc::new(tls))));
        let listener = TcpListener::bind(("0.0.0.0", port))
            .await
            .with_context(|| format!("failed to listen for NBD on port {port}"))?;
        let port = listener.local_addr()?.port();
        let serving = server.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        let server = serving.clone();
                        tokio::spawn(async move {
                            if let Err(e) = server.serve(stream).await {
                                debug!(%peer, error = %e, "NBD connection ended");
                            }
                        });
                    }
                    // Out of file descriptors, say: the next accept may work.
                    Err(e) => {
                        warn!(error = %e, "failed to accept an NBD connection");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
        info!("Serving NBD on port {port}");
        Ok(Self {
            server,
            port,
            tls_dir: tls_dir.to_path_buf(),
            offered: Mutex::default(),
        })
    }

    /// Offer the files of the snapshot directory at `snapshot_url` as
    /// "<export_id>/<path>"; returns their paths.
    fn export(&self, export_id: &str, snapshot_url: &str) -> Result<Vec<String>, CopyError> {
        let dir = snapshot::snapshot_dir(snapshot_url)
            .map_err(|why| CopyError::Invalid(format!("snapshot {snapshot_url}: {why}")))?;
        if !snapshot::is_snapshot_dir(dir) {
            return Err(CopyError::Invalid(format!(
                "{} is not a snapshot directory",
                dir.display()
            )));
        }
        let files = files_in(dir)
            .map_err(|e| CopyError::Failed(format!("failed to list {}: {e}", dir.display())))?;
        let backends = files
            .iter()
            .map(|file| {
                FileBackend::open(dir.join(file), Access::ReadOnly)
                    .map_err(|e| CopyError::Failed(format!("failed to open {file}: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut offered = self.offered.lock().unwrap();
        if offered.contains_key(export_id) {
            return Err(CopyError::Exists(format!(
                "export {export_id} is offered already"
            )));
        }
        let names = files
            .iter()
            .zip(backends)
            .map(|(file, backend)| {
                let name = format!("{export_id}/{file}");
                self.server.export(name.clone(), Arc::new(backend));
                name
            })
            .collect();
        offered.insert(export_id.to_string(), names);
        info!("Exporting snapshot {} as {export_id}", dir.display());
        Ok(files)
    }

    /// Withdraw the exports of `export_id`. Connected clients keep theirs.
    fn unexport(&self, export_id: &str) {
        let names = self.offered.lock().unwrap().remove(export_id);
        for name in names.iter().flatten() {
            self.server.unexport(name);
        }
    }

    /// Copy another node's exports of a snapshot into the new directory at
    /// the request's `snapshot_url`; returns the bytes of data copied.
    async fn pull(&self, req: PullSnapshotRequest) -> Result<u64, CopyError> {
        let dir = snapshot::snapshot_dir(&req.snapshot_url)
            .map_err(|why| CopyError::Invalid(format!("snapshot {}: {why}", req.snapshot_url)))?
            .to_path_buf();
        if let Some(file) = req.files.iter().find(|file| !is_plain_relative(file)) {
            return Err(CopyError::Invalid(format!(
                "{file} is not a path within a snapshot directory"
            )));
        }
        let port = u16::try_from(req.port)
            .map_err(|_| CopyError::Invalid(format!("port {}", req.port)))?;
        let server_name = ServerName::try_from(req.address.clone())
            .map_err(|e| CopyError::Invalid(format!("address {}: {e}", req.address)))?;
        let tls = crate::tls::nbd_client_config(&self.tls_dir)
            .map_err(|e| CopyError::Precondition(format!("{e:#}")))?;
        let connector = TlsConnector::from(Arc::new(tls));

        if let Some(parent) = dir.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                CopyError::Failed(format!("failed to create {}: {e}", parent.display()))
            })?;
        }
        // A new directory: a copy never writes over another snapshot.
        tokio::fs::create_dir(&dir)
            .await
            .map_err(|e| match e.kind() {
                io::ErrorKind::AlreadyExists => {
                    CopyError::Exists(format!("{} exists already", dir.display()))
                }
                _ => CopyError::Failed(format!("failed to create {}: {e}", dir.display())),
            })?;
        let pulled = async {
            let mut bytes = 0;
            for file in &req.files {
                let target = dir.join(file);
                if let Some(parent) = target.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let out = File::create_new(&target)
                    .with_context(|| format!("failed to create {}", target.display()))?;
                let synced = out.try_clone()?;
                let stream = TcpStream::connect((req.address.as_str(), port))
                    .await
                    .with_context(|| format!("failed to reach {}:{port}", req.address))?;
                let export = format!("{}/{file}", req.export_id);
                let mut client =
                    Client::connect_tls(stream, &export, &connector, server_name.clone()).await?;
                bytes += client.copy_to(out).await?;
                client.disconnect().await?;
                // A backup is only a backup once it is on disk.
                tokio::task::spawn_blocking(move || synced.sync_all()).await??;
            }
            sync_dirs(&dir, &req.files).await?;
            anyhow::Ok(bytes)
        }
        .await;
        match pulled {
            Ok(bytes) => {
                info!(
                    "Copied snapshot {} from {} into {} ({bytes} bytes of data)",
                    req.export_id,
                    req.address,
                    dir.display()
                );
                Ok(bytes)
            }
            Err(e) => {
                if let Err(cleanup) = tokio::fs::remove_dir_all(&dir).await {
                    warn!(error = %cleanup, "failed to remove the partial copy {}", dir.display());
                }
                Err(CopyError::Failed(format!(
                    "failed to copy snapshot {} from {}: {e:#}",
                    req.export_id, req.address
                )))
            }
        }
    }
}

/// The paths of the regular files in `dir` and the directories in it, with
/// `/` between their components, sorted. A hypervisor writes nothing else
/// in a snapshot directory.
fn files_in(dir: &Path) -> io::Result<Vec<String>> {
    let mut files = Vec::new();
    let mut dirs = vec![PathBuf::new()];
    while let Some(relative) = dirs.pop() {
        for entry in std::fs::read_dir(dir.join(&relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            let kind = entry.file_type()?;
            if kind.is_dir() {
                dirs.push(path);
            } else if kind.is_file() {
                let path = path.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "a file name that isn't UTF-8")
                })?;
                files.push(path.to_string());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Whether `path` names something inside a directory: relative, with no
/// `..`.
fn is_plain_relative(path: &str) -> bool {
    let path = Path::new(path);
    path.components().count() > 0 && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Make the copied files' directory entries durable: those of `dir` and of
/// each directory in it that holds one of `files`.
async fn sync_dirs(dir: &Path, files: &[String]) -> io::Result<()> {
    let mut dirs: Vec<PathBuf> = files
        .iter()
        .filter_map(|file| dir.join(file).parent().map(Path::to_path_buf))
        .collect();
    dirs.push(dir.to_path_buf());
    dirs.sort();
    dirs.dedup();
    tokio::task::spawn_blocking(move || dirs.iter().try_for_each(|dir| File::open(dir)?.sync_all()))
        .await?
}

#[tonic::async_trait]
impl SnapshotCopyService for SnapshotCopyServiceImpl {
    async fn export_snapshot(
        &self,
        request: Request<ExportSnapshotRequest>,
    ) -> Result<Response<ExportSnapshotResponse>, Status> {
        let req = request.into_inner();
        let files = self.export(&req.export_id, &req.snapshot_url)?;
        Ok(Response::new(ExportSnapshotResponse {
            port: u32::from(self.port),
            files,
        }))
    }

    async fn unexport_snapshot(
        &self,
        request: Request<UnexportSnapshotRequest>,
    ) -> Result<Response<()>, Status> {
        self.unexport(&request.into_inner().export_id);
        Ok(Response::new(()))
    }

    async fn pull_snapshot(
        &self,
        request: Request<PullSnapshotRequest>,
    ) -> Result<Response<PullSnapshotResponse>, Status> {
        let bytes = self.pull(request.into_inner()).await?;
        Ok(Response::new(PullSnapshotResponse { bytes }))
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{FileExt, MetadataExt};

    use super::*;
    use crate::tls::tests::gen_node_tls;

    const MIB: u64 = 1024 * 1024;

    /// A Cloud Hypervisor snapshot directory in `root`: its marker files, a
    /// sparse memory file and a sparse disk copy, each with a MiB of data.
    fn snapshot_dir(root: &Path) -> PathBuf {
        let dir = root.join("snapshot");
        std::fs::create_dir_all(dir.join("disks")).unwrap();
        std::fs::write(dir.join("config.json"), b"{}").unwrap();
        std::fs::write(dir.join("state.json"), b"{}").unwrap();
        std::fs::write(dir.join("disks.json"), b"[]").unwrap();
        for (name, byte) in [("memory-ranges", 1u8), ("disks/0.img", 2)] {
            let file = File::create(dir.join(name)).unwrap();
            file.set_len(64 * MIB).unwrap();
            file.write_all_at(&vec![byte; MIB as usize], 16 * MIB)
                .unwrap();
        }
        dir
    }

    fn url(dir: &Path) -> String {
        format!("file://{}", dir.display())
    }

    fn pull_request(
        port: u16,
        export_id: &str,
        files: Vec<String>,
        to: &Path,
    ) -> PullSnapshotRequest {
        PullSnapshotRequest {
            address: "127.0.0.1".into(),
            port: u32::from(port),
            export_id: export_id.into(),
            files,
            snapshot_url: url(to),
        }
    }

    #[tokio::test]
    async fn a_node_copies_another_nodes_snapshot_holes_and_all() {
        let certs = gen_node_tls(&["127.0.0.1"]);
        let node = SnapshotCopyServiceImpl::serve(&certs.path().join("nodes/127.0.0.1"), 0)
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let source = snapshot_dir(root.path());

        let files = node.export("copy-1", &url(&source)).unwrap();
        assert_eq!(
            files,
            [
                "config.json",
                "disks.json",
                "disks/0.img",
                "memory-ranges",
                "state.json"
            ]
        );
        let target = root.path().join("pool/copy");
        let bytes = node
            .pull(pull_request(node.port, "copy-1", files.clone(), &target))
            .await
            .unwrap();
        // The two MiB of data and the small files, not 128 MiB.
        assert!((2 * MIB..3 * MIB).contains(&bytes), "{bytes}");
        for file in &files {
            assert!(
                std::fs::read(target.join(file)).unwrap()
                    == std::fs::read(source.join(file)).unwrap(),
                "{file} differs"
            );
        }
        assert!(
            std::fs::metadata(target.join("disks/0.img"))
                .unwrap()
                .blocks()
                * 512
                < 4 * MIB
        );

        // Once withdrawn, the exports can't be pulled, and a failed pull
        // leaves nothing behind.
        node.unexport("copy-1");
        let again = root.path().join("pool/again");
        let error = node
            .pull(pull_request(node.port, "copy-1", files, &again))
            .await
            .unwrap_err();
        assert!(matches!(error, CopyError::Failed(_)), "{error}");
        assert!(!again.exists());
    }

    #[tokio::test]
    async fn only_snapshots_are_exported_and_copies_stay_in_their_directory() {
        let certs = gen_node_tls(&["127.0.0.1"]);
        let node = SnapshotCopyServiceImpl::serve(&certs.path().join("nodes/127.0.0.1"), 0)
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();

        let error = node.export("other", &url(root.path())).unwrap_err();
        assert!(matches!(error, CopyError::Invalid(_)), "{error}");

        let request = pull_request(
            node.port,
            "x",
            vec!["../escape".into()],
            &root.path().join("c"),
        );
        let error = node.pull(request).await.unwrap_err();
        assert!(matches!(error, CopyError::Invalid(_)), "{error}");
        assert!(!root.path().join("c").exists());
    }
}
