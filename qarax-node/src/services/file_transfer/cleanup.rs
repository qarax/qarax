use std::path::{Component, Path, PathBuf};

use tonic::Status;
use uuid::Uuid;

use crate::rpc::node::{DeleteStorageFilesRequest, StoragePoolKind};

/// Derive filenames instead of accepting an arbitrary deletion path. Never
/// recurse or follow a final symlink; remove_file only unlinks the file itself.
#[allow(clippy::result_large_err)]
pub(super) async fn paths(req: DeleteStorageFilesRequest) -> Result<Vec<PathBuf>, Status> {
    let pool_id =
        Uuid::parse_str(&req.pool_id).map_err(|_| Status::invalid_argument("Invalid pool ID"))?;
    let object_id = Uuid::parse_str(&req.object_id)
        .map_err(|_| Status::invalid_argument("Invalid object ID"))?;
    let root = match StoragePoolKind::try_from(req.pool_kind) {
        Ok(StoragePoolKind::Local) => {
            let config: serde_json::Value = serde_json::from_str(&req.config_json)
                .map_err(|_| Status::invalid_argument("Invalid pool config"))?;
            PathBuf::from(
                config
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Status::invalid_argument("Missing pool path"))?,
            )
        }
        Ok(StoragePoolKind::Nfs) => {
            let root = PathBuf::from(format!("/var/lib/qarax/pools/{pool_id}"));
            // Missing files on an unmounted NFS pool do not prove deletion.
            let mounts = tokio::fs::read_to_string("/proc/self/mountinfo")
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            if !mounts
                .lines()
                .any(|line| line.split_whitespace().nth(4) == root.to_str())
            {
                return Err(Status::failed_precondition("NFS pool is not mounted"));
            }
            root
        }
        _ => {
            return Err(Status::invalid_argument(
                "Cleanup requires a Local or NFS pool",
            ));
        }
    };
    if !root.is_absolute() || root.components().any(|c| c == Component::ParentDir) {
        return Err(Status::invalid_argument(
            "Pool path must be absolute without traversal",
        ));
    }
    let names = if req.upper_layer {
        vec![
            format!("{object_id}.upper.data"),
            format!("{object_id}.upper.index"),
        ]
    } else {
        vec![object_id.to_string(), format!("{object_id}.tmp")]
    };
    Ok(names.into_iter().map(|name| root.join(name)).collect())
}

#[allow(clippy::result_large_err)]
pub(super) async fn unlink(path: &Path) -> Result<(), Status> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Status::internal(format!(
            "Cannot remove {}: {err}",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn delete(req: DeleteStorageFilesRequest) -> Result<(), Status> {
        for path in paths(req).await? {
            unlink(&path).await?;
        }
        Ok(())
    }

    fn request(root: &Path, upper_layer: bool) -> DeleteStorageFilesRequest {
        DeleteStorageFilesRequest {
            pool_id: Uuid::new_v4().to_string(),
            object_id: Uuid::new_v4().to_string(),
            pool_kind: StoragePoolKind::Local as i32,
            config_json: serde_json::json!({"path": root}).to_string(),
            upper_layer,
        }
    }

    #[tokio::test]
    async fn deletes_owned_files_idempotently_and_preserves_neighbors() {
        let dir = tempfile::tempdir().unwrap();
        for upper in [false, true] {
            let req = request(dir.path(), upper);
            let suffixes = if upper {
                vec![".upper.data", ".upper.index"]
            } else {
                vec!["", ".tmp"]
            };
            for suffix in &suffixes {
                tokio::fs::write(
                    dir.path().join(format!("{}{suffix}", req.object_id)),
                    b"data",
                )
                .await
                .unwrap();
            }
            let external = dir.path().join("external.img");
            tokio::fs::write(&external, b"keep").await.unwrap();
            delete(req.clone()).await.unwrap();
            delete(req.clone()).await.unwrap();
            for suffix in suffixes {
                assert!(
                    !dir.path()
                        .join(format!("{}{suffix}", req.object_id))
                        .exists()
                );
            }
            assert_eq!(tokio::fs::read(external).await.unwrap(), b"keep");
        }
    }

    #[tokio::test]
    async fn refuses_directories_traversal_and_unmounted_nfs() {
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(dir.path(), false);
        tokio::fs::create_dir(dir.path().join(&req.object_id))
            .await
            .unwrap();
        assert!(delete(req.clone()).await.is_err());
        req.object_id = "../external".into();
        assert_eq!(
            delete(req.clone()).await.unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        req.object_id = Uuid::new_v4().to_string();
        req.config_json = r#"{"path":"/tmp/../etc"}"#.into();
        assert_eq!(
            delete(req.clone()).await.unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        req.pool_kind = StoragePoolKind::Nfs as i32;
        assert_eq!(
            delete(req).await.unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
    }
}
