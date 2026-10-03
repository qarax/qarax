//! Removal of VM snapshot directories written by `SnapshotVM`.
//!
//! The control plane only sends a `file://` URL, so before deleting anything
//! we check that the target really is a snapshot directory: an absolute path
//! without `..` that contains the files one of our hypervisors writes.

use std::path::{Component, Path};

use thiserror::Error;
use tracing::info;

/// Files Cloud Hypervisor writes for `vm.snapshot`.
const CLOUD_HV_MARKERS: &[&str] = &["config.json", "state.json"];
/// Files Firecracker writes for `create_snapshot` (see `firecracker::snapshot_vm`).
const FIRECRACKER_MARKERS: &[&str] = &["mem.snap", "vm.snap"];

#[derive(Debug, Error)]
pub enum SnapshotDeleteError {
    #[error("invalid snapshot path {0}: {1}")]
    InvalidPath(String, &'static str),
    #[error("{0} is not a snapshot directory")]
    NotASnapshot(String),
    #[error("failed to remove snapshot {0}: {1}")]
    Io(String, #[source] std::io::Error),
}

/// Delete the snapshot directory at `snapshot_url` (`file:///abs/path` or a
/// bare absolute path). Returns `Ok(false)` if it does not exist.
pub async fn delete_snapshot_dir(snapshot_url: &str) -> Result<bool, SnapshotDeleteError> {
    let raw = snapshot_url.strip_prefix("file://").unwrap_or(snapshot_url);
    let path = Path::new(raw);

    if !path.is_absolute() {
        return Err(SnapshotDeleteError::InvalidPath(
            raw.into(),
            "must be absolute",
        ));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(SnapshotDeleteError::InvalidPath(
            raw.into(),
            "must not contain '..'",
        ));
    }
    if path.parent().is_none() {
        return Err(SnapshotDeleteError::InvalidPath(
            raw.into(),
            "refusing to delete the root directory",
        ));
    }

    match tokio::fs::symlink_metadata(path).await {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(SnapshotDeleteError::NotASnapshot(raw.into())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(SnapshotDeleteError::Io(raw.into(), e)),
    }

    let has_all = |markers: &[&str]| markers.iter().all(|m| path.join(m).is_file());
    if !has_all(CLOUD_HV_MARKERS) && !has_all(FIRECRACKER_MARKERS) {
        return Err(SnapshotDeleteError::NotASnapshot(raw.into()));
    }

    tokio::fs::remove_dir_all(path)
        .await
        .map_err(|e| SnapshotDeleteError::Io(raw.into(), e))?;
    info!("Deleted snapshot directory {}", raw);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn snapshot_dir(root: &TempDir, name: &str, files: &[&str]) -> String {
        let dir = root.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        dir.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn deletes_cloud_hypervisor_snapshot() {
        let root = TempDir::new().unwrap();
        let dir = snapshot_dir(&root, "ch", &["config.json", "state.json", "memory-ranges"]);
        assert!(delete_snapshot_dir(&format!("file://{dir}")).await.unwrap());
        assert!(!Path::new(&dir).exists());
    }

    #[tokio::test]
    async fn deletes_firecracker_snapshot() {
        let root = TempDir::new().unwrap();
        let dir = snapshot_dir(&root, "fc", &["mem.snap", "vm.snap"]);
        assert!(delete_snapshot_dir(&dir).await.unwrap());
        assert!(!Path::new(&dir).exists());
    }

    #[tokio::test]
    async fn missing_directory_is_not_an_error() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("gone");
        assert!(!delete_snapshot_dir(dir.to_str().unwrap()).await.unwrap());
    }

    #[tokio::test]
    async fn refuses_directory_without_snapshot_files() {
        let root = TempDir::new().unwrap();
        let dir = snapshot_dir(&root, "other", &["config.json", "important.db"]);
        assert!(matches!(
            delete_snapshot_dir(&dir).await,
            Err(SnapshotDeleteError::NotASnapshot(_))
        ));
        assert!(Path::new(&dir).exists());
    }

    #[tokio::test]
    async fn refuses_unsafe_paths() {
        for bad in ["relative/dir", "/var/lib/qarax/../../etc", "/", "file:///"] {
            assert!(
                matches!(
                    delete_snapshot_dir(bad).await,
                    Err(SnapshotDeleteError::InvalidPath(..))
                ),
                "{bad} should be rejected"
            );
        }
    }
}
