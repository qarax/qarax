//! VM snapshot directories, written by `SnapshotVM`: the VM's disks captured
//! with its memory, put back when the VM is restored, and the directories'
//! removal.
//!
//! A hypervisor's snapshot holds the VM's memory and device state but not its
//! disks, which it reopens where they are when the VM is restored. Restored
//! over disks the guest wrote to since, its memory (the page cache, the
//! filesystems' state) wouldn't match them. So a snapshot captures each
//! writable disk while the VM is paused, a copy (a reflink where the
//! filesystem has them) in the snapshot's directory, and a restore puts the
//! copies back first.
//!
//! The control plane only sends a `file://` URL, so before deleting anything
//! we check that the target really is a snapshot directory: an absolute path
//! without `..` that contains the files one of our hypervisors writes.

use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

use crate::disk_image;
use crate::rpc::node::{ImageFormat, VmConfig};
use crate::storage::files;

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

/// The directory `snapshot_url` (`file:///abs/path` or a bare absolute path)
/// names, or why it can't be a snapshot's.
fn snapshot_dir(snapshot_url: &str) -> Result<&Path, &'static str> {
    let path = Path::new(snapshot_url.trim_prefix("file://"));
    if !path.is_absolute() {
        return Err("must be absolute");
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("must not contain '..'");
    }
    if path.parent().is_none() {
        return Err("must not be the root directory");
    }
    Ok(path)
}

/// Delete the snapshot directory at `snapshot_url` (`file:///abs/path` or a
/// bare absolute path). Returns `Ok(false)` if it does not exist.
pub async fn delete_snapshot_dir(snapshot_url: &str) -> Result<bool, SnapshotDeleteError> {
    let raw = snapshot_url.trim_prefix("file://");
    let path = snapshot_dir(snapshot_url)
        .map_err(|why| SnapshotDeleteError::InvalidPath(raw.into(), why))?;

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

/// The list of the disks captured with a snapshot, in its directory. Written
/// after the copies, so that it lists only whole ones.
const DISKS_FILE: &str = "disks.json";
/// The directory of the copies in a snapshot's: `<n>.img` is the copy of the
/// `n`th disk listed.
const DISKS_DIR: &str = "disks";

/// A disk captured with a snapshot.
#[derive(Debug, Serialize, Deserialize)]
struct CapturedDisk {
    /// Its image file, which a restore puts the copy back over.
    path: String,
}

#[derive(Debug, Error)]
pub enum SnapshotDisksError {
    /// The VM has a writable disk that a snapshot can't capture.
    #[error("{0}")]
    Uncapturable(String),
    #[error("invalid snapshot path {0}: {1}")]
    InvalidPath(String, &'static str),
    #[error("{0}: {1}")]
    Io(String, #[source] io::Error),
}

fn io_error(what: impl Into<String>) -> impl FnOnce(io::Error) -> SnapshotDisksError {
    let what = what.into();
    move |e| SnapshotDisksError::Io(what, e)
}

/// The image files of `config`'s writable disks, which a snapshot of the VM
/// captures; its read-only disks stay as they are. Refuses a VM with a
/// writable disk that can't be copied whole while the VM is paused: an OCI
/// image's disk, whose writable layer OverlayBD holds open; a disk that isn't
/// a file, such as an iSCSI LUN; and a qcow2 disk, whose metadata Cloud
/// Hypervisor keeps in memory, writing it to the file only when the guest
/// flushes the disk.
pub fn disks_to_capture(config: &VmConfig) -> Result<Vec<String>, SnapshotDisksError> {
    let writable = config.disks.iter().filter(|d| !d.readonly.unwrap_or(false));
    writable
        .map(|disk| {
            let refuse = |why: &str| {
                SnapshotDisksError::Uncapturable(format!(
                    "can't snapshot the VM with its disk {}: {why}",
                    disk.id
                ))
            };
            if disk.oci_image_ref.is_some() {
                return Err(refuse(
                    "it is an OCI image's, whose writable layer snapshots don't capture yet",
                ));
            }
            let Some(path) = disk.path.as_deref() else {
                return Err(refuse("it has no image file"));
            };
            let metadata = std::fs::metadata(path).map_err(io_error(path))?;
            if !metadata.is_file() {
                return Err(refuse(
                    "it isn't an image file (an iSCSI LUN?), which only its storage can snapshot",
                ));
            }
            if disk_image::recorded_or_probed(disk.image_format(), Path::new(path))
                == ImageFormat::Qcow2
            {
                return Err(refuse(
                    "it is qcow2, whose metadata the hypervisor keeps in memory, so a copy could miss what the guest wrote",
                ));
            }
            Ok(path.to_string())
        })
        .collect()
}

/// Copy each of `disks` into the snapshot directory at `snapshot_url`, then
/// list them there for [`restore_disks`]. Call it with the VM paused, once its
/// hypervisor has written the snapshot: the copies hold what the guest wrote
/// up to the pause. A copy is a reflink where the filesystem has them, and
/// takes as long as copying the disk's data elsewhere.
pub async fn capture_disks(
    snapshot_url: &str,
    disks: Vec<String>,
) -> Result<(), SnapshotDisksError> {
    let dir = snapshot_dir(snapshot_url)
        .map_err(|why| SnapshotDisksError::InvalidPath(snapshot_url.into(), why))?
        .to_path_buf();
    blocking(move || {
        let copies = dir.join(DISKS_DIR);
        std::fs::create_dir(&copies).map_err(io_error(copies.display().to_string()))?;
        for (n, path) in disks.iter().enumerate() {
            files::copy_sparse(Path::new(path), &copies.join(format!("{n}.img")))
                .map_err(io_error(format!("copying {path}")))?;
        }
        let count = disks.len();
        let list: Vec<CapturedDisk> = disks
            .into_iter()
            .map(|path| CapturedDisk { path })
            .collect();
        let list = serde_json::to_vec(&list).expect("a list of strings serializes");
        let (staging, file) = (dir.join(format!("{DISKS_FILE}.new")), dir.join(DISKS_FILE));
        std::fs::write(&staging, list).map_err(io_error(format!("writing {DISKS_FILE}")))?;
        std::fs::rename(&staging, &file).map_err(io_error(format!("writing {DISKS_FILE}")))?;
        info!("Captured {count} disk(s) in {}", dir.display());
        Ok(())
    })
    .await
}

/// Put back the disks captured with the snapshot at `snapshot_url`, each copy
/// over the disk it was taken of. Call it before the hypervisor restores the
/// VM, with the VM not running.
pub async fn restore_disks(snapshot_url: &str) -> Result<(), SnapshotDisksError> {
    let dir = snapshot_dir(snapshot_url)
        .map_err(|why| SnapshotDisksError::InvalidPath(snapshot_url.into(), why))?
        .to_path_buf();
    blocking(move || {
        let list = match std::fs::read(dir.join(DISKS_FILE)) {
            Ok(list) => list,
            // A snapshot from before snapshots captured disks: its VM gets back
            // its memory and devices only, as such a snapshot always did.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io_error(format!("reading {DISKS_FILE}"))(e)),
        };
        let disks: Vec<CapturedDisk> = serde_json::from_slice(&list)
            .map_err(|e| io_error(format!("reading {DISKS_FILE}"))(io::Error::other(e)))?;
        for (n, disk) in disks.iter().enumerate() {
            // The disk must still be there: a copy put back where a deleted
            // disk was would be a file nothing owns.
            let permissions = std::fs::metadata(&disk.path)
                .map_err(io_error(format!("the captured disk {}", disk.path)))?
                .permissions();
            let staging = PathBuf::from(format!("{}.restoring", disk.path));
            match std::fs::remove_file(&staging) {
                // Left by a restore that didn't finish.
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_error(staging.display().to_string())(e)),
            }
            let copy = dir.join(DISKS_DIR).join(format!("{n}.img"));
            files::copy_sparse(&copy, &staging)
                .map_err(io_error(format!("restoring {}", disk.path)))?;
            std::fs::set_permissions(&staging, permissions)
                .map_err(io_error(format!("restoring {}", disk.path)))?;
            std::fs::rename(&staging, &disk.path)
                .map_err(io_error(format!("restoring {}", disk.path)))?;
        }
        info!("Restored {} disk(s) from {}", disks.len(), dir.display());
        Ok(())
    })
    .await
}

async fn blocking(
    op: impl FnOnce() -> Result<(), SnapshotDisksError> + Send + 'static,
) -> Result<(), SnapshotDisksError> {
    match tokio::task::spawn_blocking(op).await {
        Ok(result) => result,
        Err(e) => std::panic::resume_unwind(e.into_panic()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::node::DiskConfig;
    use std::assert_matches;
    use std::os::unix::fs::PermissionsExt;
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
        assert_matches!(
            delete_snapshot_dir(&dir).await,
            Err(SnapshotDeleteError::NotASnapshot(_))
        );
        assert!(Path::new(&dir).exists());
    }

    #[tokio::test]
    async fn refuses_unsafe_paths() {
        for bad in ["relative/dir", "/var/lib/qarax/../../etc", "/", "file:///"] {
            assert_matches!(
                delete_snapshot_dir(bad).await,
                Err(SnapshotDeleteError::InvalidPath(..)),
                "{bad} should be rejected"
            );
        }
    }

    fn disk(id: &str, path: &Path) -> DiskConfig {
        DiskConfig {
            id: id.into(),
            path: Some(path.display().to_string()),
            ..Default::default()
        }
    }

    fn vm(disks: Vec<DiskConfig>) -> VmConfig {
        VmConfig {
            disks,
            ..Default::default()
        }
    }

    #[test]
    fn a_snapshot_captures_the_writable_raw_disks() {
        let dir = TempDir::new().unwrap();
        let (data, image) = (dir.path().join("data.raw"), dir.path().join("image.qcow2"));
        std::fs::write(&data, b"data").unwrap();
        std::fs::write(&image, b"QFI\xfb").unwrap();
        let read_only = DiskConfig {
            readonly: Some(true),
            ..disk("image", &image)
        };

        let disks = disks_to_capture(&vm(vec![read_only, disk("data", &data)])).unwrap();
        assert_eq!(disks, [data.display().to_string()]);
    }

    #[test]
    fn a_writable_disk_that_cant_be_copied_whole_refuses_the_snapshot() {
        let dir = TempDir::new().unwrap();
        let raw = dir.path().join("disk.raw");
        std::fs::write(&raw, b"raw").unwrap();
        let cases = [
            DiskConfig {
                image_format: ImageFormat::Qcow2 as i32,
                ..disk("qcow2", &raw)
            },
            DiskConfig {
                oci_image_ref: Some("registry:5000/alpine:latest".into()),
                ..disk("oci", Path::new("/dev/sda"))
            },
            // A block device, as an iSCSI LUN is.
            disk("lun", Path::new("/dev/null")),
            DiskConfig {
                path: None,
                ..disk("vhost", &raw)
            },
        ];
        for case in cases {
            let id = case.id.clone();
            match disks_to_capture(&vm(vec![case])) {
                Err(SnapshotDisksError::Uncapturable(message)) => {
                    assert!(message.contains(&id), "{message}")
                }
                other => panic!("disk {id}: {other:?}"),
            }
        }
    }

    /// A 1 MiB raw disk that starts with `contents`, readable by its owner
    /// only, and the URL of an empty snapshot directory.
    fn disk_and_snapshot(dir: &TempDir, contents: &[u8]) -> (PathBuf, String) {
        let disk = dir.path().join("disk.raw");
        std::fs::write(&disk, contents).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&disk).unwrap();
        file.set_len(1024 * 1024).unwrap();
        std::fs::set_permissions(&disk, std::fs::Permissions::from_mode(0o600)).unwrap();
        let snapshot = dir.path().join("snapshot");
        std::fs::create_dir(&snapshot).unwrap();
        (disk, format!("file://{}", snapshot.display()))
    }

    #[tokio::test]
    async fn a_restore_puts_the_captured_disks_back() {
        let dir = TempDir::new().unwrap();
        let (disk, snapshot) = disk_and_snapshot(&dir, b"before");
        capture_disks(&snapshot, vec![disk.display().to_string()])
            .await
            .unwrap();
        std::fs::write(&disk, b"after!").unwrap();

        restore_disks(&snapshot).await.unwrap();

        let restored = std::fs::read(&disk).unwrap();
        assert_eq!(&restored[..6], b"before");
        assert_eq!(restored.len(), 1024 * 1024);
        let mode = std::fs::metadata(&disk).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(!Path::new(&format!("{}.restoring", disk.display())).exists());
    }

    #[tokio::test]
    async fn a_snapshot_from_before_disk_capture_restores_no_disk() {
        let dir = TempDir::new().unwrap();
        let (disk, snapshot) = disk_and_snapshot(&dir, b"current");

        restore_disks(&snapshot).await.unwrap();

        assert_eq!(&std::fs::read(&disk).unwrap()[..7], b"current");
    }

    #[tokio::test]
    async fn a_restore_needs_the_captured_disk_still_there() {
        let dir = TempDir::new().unwrap();
        let (disk, snapshot) = disk_and_snapshot(&dir, b"before");
        capture_disks(&snapshot, vec![disk.display().to_string()])
            .await
            .unwrap();
        std::fs::remove_file(&disk).unwrap();

        assert_matches!(
            restore_disks(&snapshot).await,
            Err(SnapshotDisksError::Io(..))
        );
        assert!(!disk.exists());
    }
}
