//! A seed's snapshot: what the forks of a seed, a VM forked as a sandbox
//! pool's, load. A seed's first fork as a seed pauses it and takes the
//! snapshot, as a fork of its current state would, and the node keeps it; its
//! forks after that load the kept snapshot without touching the seed. So they
//! all continue from that one moment of the seed, and map one memory image,
//! whose page cache they share, instead of one image per fork round.
//!
//! The snapshot is the hypervisor's (the seed's memory and device state), the
//! seed's config then, its OCI image disk's writable layer as it was, sealed,
//! and a copy of each of its data disks. Its files live in the runtime
//! directory's `seeds/` (the sealed layer in OverlayBD's cache) until the last
//! of its users lets go of it: the seed holds it until it stops or is deleted,
//! and a fork round in progress holds it until its forks have loaded. The
//! forks don't need them after that: a Firecracker fork maps the memory image,
//! which outlives its name; a Cloud Hypervisor fork, an OCI disk and a data
//! disk copy have files of their own.

use std::path::{Path, PathBuf};

use tracing::warn;

use super::VmmError;
use super::fork::{DataDisk, ForkDisks, SourceDisk, blocking, data_disks, mount_fork_disks};
use crate::overlaybd::FrozenUpper;
use crate::rpc::node::{Fork, VmConfig as ProtoVmConfig};

/// The runtime directory's directory of seed snapshots.
const SEEDS_DIR: &str = "seeds";

/// Remove the seed snapshots of the node's last run, which no seed has any
/// more: a seed's next fork as a seed takes a new one.
pub async fn remove_stale_snapshots(runtime_dir: &Path) {
    let seeds = runtime_dir.join(SEEDS_DIR);
    if let Err(e) = tokio::fs::remove_dir_all(&seeds).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!(
            "Failed to remove the seed snapshots in {}: {}",
            seeds.display(),
            e
        );
    }
}

/// The directory of a seed's snapshot, removed with it: one being taken, and
/// then the [`SeedSnapshot`]'s.
pub(crate) struct SeedDir(PathBuf);

impl SeedDir {
    /// A new directory for a snapshot of `seed_id`, in `runtime_dir`.
    pub(crate) async fn new(runtime_dir: &Path, seed_id: &str) -> Result<Self, VmmError> {
        let dir = runtime_dir
            .join(SEEDS_DIR)
            .join(format!("{seed_id}-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(dir.join("vm"))
            .await
            .map_err(|e| {
                VmmError::ProcessError(format!(
                    "Failed to create the seed snapshot directory {}: {e}",
                    dir.display()
                ))
            })?;
        Ok(SeedDir(dir))
    }

    /// Where the hypervisor writes its snapshot of the seed.
    pub(crate) fn vm(&self) -> PathBuf {
        self.0.join("vm")
    }

    /// The seed's disks to keep: `oci`, its OCI image disk, and `data`, its
    /// data disks, each copied into this directory. Returns the copies too.
    pub(crate) fn disks<'a>(
        &self,
        oci: Option<&'a SourceDisk>,
        data: &[DataDisk],
    ) -> (ForkDisks<'a>, Vec<DataDisk>) {
        let kept: Vec<DataDisk> = data
            .iter()
            .enumerate()
            .map(|(i, disk)| DataDisk {
                drive_id: disk.drive_id.clone(),
                path: self.0.join(format!("disk-{i}")),
            })
            .collect();
        let copies = data
            .iter()
            .zip(&kept)
            .map(|(disk, copy)| (disk.path.clone(), vec![copy.path.clone()]))
            .collect();
        (ForkDisks::with_copies(oci, copies), kept)
    }

    /// The snapshot of the seed, whose config was `config`, once the
    /// hypervisor has written it here and the pause froze `frozen`, the OCI
    /// disk's writable layer, which this seals; `data` are the copies
    /// [`Self::disks`] made.
    pub(crate) async fn keep(
        self,
        config: ProtoVmConfig,
        oci: Option<SourceDisk>,
        frozen: Option<FrozenUpper>,
        data: Vec<DataDisk>,
    ) -> Result<SeedSnapshot, VmmError> {
        let oci = match (oci, frozen) {
            (Some(disk), Some(frozen)) => {
                disk.overlaybd
                    .seal(&frozen)
                    .await
                    .map_err(super::fork::overlaybd_err)?;
                Some((disk, frozen))
            }
            _ => None,
        };
        Ok(SeedSnapshot {
            config,
            dir: self,
            oci,
            data,
        })
    }
}

impl Drop for SeedDir {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            warn!("Failed to remove seed snapshot {}: {}", self.0.display(), e);
        }
    }
}

/// A seed's snapshot (see the module's docs).
pub(crate) struct SeedSnapshot {
    /// The seed's config when it was taken: its forks' configs are made from
    /// it.
    pub(crate) config: ProtoVmConfig,
    dir: SeedDir,
    /// The seed's OCI image disk, and its writable layer as it was, sealed.
    oci: Option<(SourceDisk, FrozenUpper)>,
    /// Its data disks, each the copy in `dir`.
    data: Vec<DataDisk>,
}

impl SeedSnapshot {
    /// The hypervisor's snapshot of the seed.
    pub(crate) fn vm(&self) -> PathBuf {
        self.dir.vm()
    }

    /// The seed's OCI image disk, which each fork gets a disk of its own on.
    pub(crate) fn oci_disk(&self) -> Option<&SourceDisk> {
        self.oci.as_ref().map(|(disk, _)| disk)
    }

    /// The seed's data disks, each fork's copied from the snapshot's.
    pub(crate) fn data(&self) -> &[DataDisk] {
        &self.data
    }

    /// Give each of `forks`, whose configs are `configs`, its disks: a copy of
    /// each data disk as it was, and a disk of its own on the OCI disk's
    /// sealed layer. Returns each fork's OCI disk device, in order. The forks
    /// must copy the disks the snapshot has.
    pub(crate) async fn give_disks(
        &self,
        forks: &[Fork],
        configs: &[ProtoVmConfig],
    ) -> Result<Vec<Option<String>>, VmmError> {
        let ids = |disks: &[DataDisk]| {
            let mut ids: Vec<String> = disks.iter().map(|disk| disk.drive_id.clone()).collect();
            ids.sort();
            ids
        };
        let copied = ids(&data_disks(&self.config, forks)?);
        if copied != ids(&self.data) {
            return Err(VmmError::InvalidConfig(format!(
                "the forks copy disks {copied:?}, and the seed's snapshot has {:?}",
                ids(&self.data)
            )));
        }
        for disk in &self.data {
            for config in configs {
                let copy = config
                    .disks
                    .iter()
                    .find(|drive| drive.id == disk.drive_id)
                    .and_then(|drive| drive.path.clone())
                    .ok_or_else(|| {
                        VmmError::InvalidConfig(format!(
                            "fork {} has no file for disk {}",
                            config.vm_id, disk.drive_id
                        ))
                    })?;
                let from = disk.path.clone();
                blocking("copying a data disk", move || {
                    crate::storage::files::copy_sparse(&from, Path::new(&copy))
                })
                .await?;
            }
        }
        let fork_ids: Vec<String> = configs.iter().map(|config| config.vm_id.clone()).collect();
        match &self.oci {
            Some((disk, frozen)) => Ok(mount_fork_disks(&disk.overlaybd, frozen, &fork_ids)
                .await?
                .into_iter()
                .map(Some)
                .collect()),
            None => Ok(vec![None; fork_ids.len()]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::node::{DiskConfig, DiskCopy, VsockConfig};
    use crate::vmm::fork::fork_config;

    fn seed_config(disk: &Path) -> ProtoVmConfig {
        ProtoVmConfig {
            vm_id: "seed".into(),
            vsock: Some(VsockConfig::default()),
            disks: vec![DiskConfig {
                id: "data".into(),
                path: Some(disk.display().to_string()),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn fork(vm_id: &str, copies: &[(&str, &Path)]) -> Fork {
        Fork {
            vm_id: vm_id.into(),
            disks: copies
                .iter()
                .map(|(drive_id, path)| DiskCopy {
                    drive_id: drive_id.to_string(),
                    path: path.display().to_string(),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_seeds_forks_get_its_data_disks_as_they_were_at_its_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let disk = dir.path().join("seed-disk");
        std::fs::write(&disk, b"at the snapshot").unwrap();
        let config = seed_config(&disk);
        let data = vec![DataDisk {
            drive_id: "data".into(),
            path: disk.clone(),
        }];

        let seed_dir = SeedDir::new(dir.path(), "seed").await.unwrap();
        let (disks, kept) = seed_dir.disks(None, &data);
        // What the pause does, after the hypervisor's snapshot.
        assert!(disks.freeze("seed").await.unwrap().is_none());
        let snapshot = seed_dir.keep(config, None, None, kept).await.unwrap();
        std::fs::write(&disk, b"since").unwrap();

        for round in ["first", "second"] {
            let copies: Vec<_> = (0..2)
                .map(|i| dir.path().join(format!("{round}-{i}")))
                .collect();
            let forks: Vec<Fork> = copies
                .iter()
                .enumerate()
                .map(|(i, copy)| fork(&format!("{round}-{i}"), &[("data", copy)]))
                .collect();
            let configs: Vec<_> = forks
                .iter()
                .map(|fork| fork_config(&snapshot.config, fork).unwrap())
                .collect();
            let devices = snapshot.give_disks(&forks, &configs).await.unwrap();
            assert_eq!(devices, [None, None]);
            for copy in &copies {
                assert_eq!(std::fs::read(copy).unwrap(), b"at the snapshot");
            }
        }

        // Forks that copy other disks than the snapshot has.
        let without = [fork("without", &[])];
        let configs = [fork_config(&snapshot.config, &without[0]).unwrap()];
        assert!(matches!(
            snapshot.give_disks(&without, &configs).await,
            Err(VmmError::InvalidConfig(_))
        ));
    }

    #[tokio::test]
    async fn a_seeds_snapshot_files_go_with_it() {
        let runtime = tempfile::tempdir().unwrap();
        let disk = runtime.path().join("seed-disk");
        std::fs::write(&disk, b"data").unwrap();
        let data = vec![DataDisk {
            drive_id: "data".into(),
            path: disk.clone(),
        }];
        let seeds = runtime.path().join(SEEDS_DIR);

        // One that failed before it was kept.
        let failed = SeedDir::new(runtime.path(), "seed").await.unwrap();
        drop(failed);
        assert_eq!(std::fs::read_dir(&seeds).unwrap().count(), 0);

        let seed_dir = SeedDir::new(runtime.path(), "seed").await.unwrap();
        let (disks, kept) = seed_dir.disks(None, &data);
        disks.freeze("seed").await.unwrap();
        std::fs::write(seed_dir.vm().join("mem.snap"), b"memory").unwrap();
        let snapshot = seed_dir
            .keep(seed_config(&disk), None, None, kept)
            .await
            .unwrap();
        assert!(snapshot.vm().join("mem.snap").is_file());
        assert!(snapshot.data()[0].path.is_file());
        drop(snapshot);
        assert_eq!(std::fs::read_dir(&seeds).unwrap().count(), 0);

        // Those of the node's last run.
        let stale = SeedDir::new(runtime.path(), "seed").await.unwrap();
        std::mem::forget(stale);
        remove_stale_snapshots(runtime.path()).await;
        assert!(!seeds.exists());
        // The seed's own disk is not the snapshot's.
        assert!(disk.is_file());
    }
}
