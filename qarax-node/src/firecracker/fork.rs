//! Forking a Firecracker VM ([`crate::vmm::fork`] has what doesn't depend on
//! the hypervisor).
//!
//! The source is paused, snapshotted and resumed. Each fork is a new
//! Firecracker process that loads the snapshot with the `File` memory
//! backend, which maps it `MAP_PRIVATE`: the forks share the source's memory
//! pages until they write them. A fork's vsock device gets the fork's own
//! socket (the guest keeps the source's CID, which only it sees), its NICs
//! TAPs of their own (`network_overrides`), and its drives are pointed at its
//! own files before it resumes.
//!
//! A VM with the guest agent runs with Firecracker tracking the pages it
//! dirties. Its first fork takes a full snapshot, whose memory image the node
//! keeps as the VM's fork base; each fork after that writes only the pages
//! dirtied since (a diff snapshot), so the pause lasts as long as writing those
//! does, and once the VM runs again lays them over a copy of the base (a
//! reflink where the filesystem has them), which becomes the next base. A base
//! is never written to: the forks that loaded it map it. One fork round of a VM
//! runs at a time, and anything else that resets the dirty-page log (a
//! snapshot, a restart) forgets the base.
//!
//! The forks load paused and are resumed in a second step, so that their
//! firewall is in place before they run.

use std::os::unix::fs::MetadataExt;

use firecracker_rust_sdk::models::snapshot_create_params::SnapshotType;
use firecracker_rust_sdk::models::{NetworkOverride, PartialDrive, VsockOverride};
use futures::future::join_all;

use super::*;
use crate::cloud_init;
use crate::overlaybd::FrozenUpper;
use crate::vmm::fork::{
    DataDisk, ForkDisks, SourceDisk, blocking, check_forkable, data_disks, fork_config,
    give_fork_its_seed, readdress_request, source_disk, tell_fork,
};

/// Whether Firecracker tracks the pages a VM dirties: for a VM with the guest
/// agent, which can be forked, so that its forks after the first take a diff
/// snapshot.
pub(super) fn tracks_dirty_pages(config: &ProtoVmConfig) -> bool {
    config.vsock.is_some()
}

/// A running VM's [`ForkMemory`], which a fork round holds from its snapshot
/// to its new base.
pub(super) type ForkMemoryLock = Arc<tokio::sync::Mutex<ForkMemory>>;

/// What the next fork of a running VM snapshots of its memory.
pub(super) enum ForkMemory {
    /// Firecracker doesn't track the pages the VM dirties, or qarax-node
    /// doesn't know that it does (a VM recovered at startup): each fork takes
    /// a full snapshot.
    Untracked,
    /// Firecracker tracks the pages the VM dirties. `base` is its memory as
    /// the last fork round's snapshot left it, the dirty-page log reset since:
    /// the next round writes only the pages dirtied since.
    Tracked { base: Option<ForkBase> },
}

impl ForkMemory {
    /// For a VM Firecracker runs with dirty-page tracking or without.
    pub(super) fn new(tracked: bool) -> ForkMemoryLock {
        Arc::new(tokio::sync::Mutex::new(if tracked {
            ForkMemory::Tracked { base: None }
        } else {
            ForkMemory::Untracked
        }))
    }

    /// Forget the base, whose diffs no longer hold: something reset the
    /// dirty-page log.
    pub(super) fn forget_base(&mut self) {
        if let ForkMemory::Tracked { base } = self {
            *base = None;
        }
    }
}

/// The memory image a VM's last fork round left, which that round's forks
/// map. Its file is unlinked when it's replaced or forgotten, which the forks
/// mapping it don't notice.
pub(super) struct ForkBase(PathBuf);

impl Drop for ForkBase {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.0)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            warn!("FC: Failed to remove fork base {}: {}", self.0.display(), e);
        }
    }
}

/// What a fork round's snapshot wrote of its source's memory.
enum MemorySnapshot {
    /// All of it, in this file.
    Full(PathBuf),
    /// The pages dirtied since `base`, in `diff`: laid over a copy of `base`,
    /// they make `image`.
    Diff {
        base: PathBuf,
        diff: PathBuf,
        image: PathBuf,
    },
}

impl FirecrackerManager {
    pub(super) async fn fork(&self, source_id: &str, forks: &[Fork]) -> Result<(), VmmError> {
        let fork_ids: Vec<String> = forks.iter().map(|fork| fork.vm_id.clone()).collect();
        info!("FC: Forking VM {} into {:?}", source_id, fork_ids);
        if fork_ids.is_empty() {
            return Err(VmmError::InvalidConfig("no VMs to fork into".into()));
        }
        let (source_config, memory) = {
            let vms = self.vms.lock().await;
            if let Some(id) = fork_ids.iter().find(|id| vms.contains_key(*id)) {
                return Err(VmmError::VmAlreadyExists(id.clone()));
            }
            let instance = vms
                .get(source_id)
                .ok_or_else(|| VmmError::VmNotFound(source_id.to_string()))?;
            let FcPhase::Live { forking, .. } = &instance.phase else {
                return Err(VmmError::ProcessError("VM is not running".into()));
            };
            (instance.proto_config.clone(), forking.clone())
        };
        // One fork round of a VM at a time: each diffs against the last one's
        // memory image.
        let mut memory = memory.lock().await;
        check_forkable(&source_config)?;
        let configs = forks
            .iter()
            .map(|fork| fork_config(&source_config, fork))
            .collect::<Result<Vec<_>, _>>()?;
        let data = data_disks(&source_config, forks)?;
        let oci = match &self.overlaybd {
            Some(backend) if backend.has_mapping(source_id).await => {
                Some(backend.manager().clone())
            }
            _ => None,
        };
        let disk = source_disk(source_id, &source_config, &data, oci)?;

        let snapshot_dir = self
            .runtime_dir
            .join(format!("{source_id}.fork-{}", uuid::Uuid::new_v4()));
        let forked = self
            .fork_from_snapshot(
                source_id,
                disk.as_ref(),
                &data,
                configs,
                &snapshot_dir,
                &mut memory,
            )
            .await;
        // The forks keep the snapshot's files mapped, so they can go now.
        if let Err(e) = tokio::fs::remove_dir_all(&snapshot_dir).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            warn!(
                "FC: Failed to remove fork snapshot {}: {}",
                snapshot_dir.display(),
                e
            );
        }
        let forks = forked?;

        let mut vms = self.vms.lock().await;
        for (fork_id, instance) in forks {
            // Persist config for recovery, as create_vm does.
            if let Err(e) = tokio::fs::write(
                self.config_path(&fork_id),
                instance.proto_config.encode_to_vec(),
            )
            .await
            {
                warn!("FC: Failed to persist config for VM {}: {}", fork_id, e);
            }
            vms.insert(fork_id, instance);
        }
        info!(
            "FC: VM {} forked into {} VMs, paused",
            source_id,
            fork_ids.len()
        );
        Ok(())
    }

    /// Resume forks [`Self::fork`] loaded, and tell each one's guest that it
    /// is a fork and, if it has NICs, their addresses.
    pub(super) async fn resume_forks(
        &self,
        source_id: &str,
        fork_ids: &[String],
    ) -> Result<(), VmmError> {
        let forks = {
            let vms = self.vms.lock().await;
            let source = &vms
                .get(source_id)
                .ok_or_else(|| VmmError::VmNotFound(source_id.to_string()))?
                .proto_config;
            fork_ids
                .iter()
                .map(|fork_id| {
                    let fork = &vms
                        .get(fork_id)
                        .ok_or_else(|| VmmError::VmNotFound(fork_id.clone()))?
                        .proto_config;
                    Ok((
                        fork_id.as_str(),
                        self.vsock_socket_path(fork_id),
                        readdress_request(source, fork)?,
                    ))
                })
                .collect::<Result<Vec<_>, VmmError>>()?
        };
        join_all(
            forks
                .into_iter()
                .map(|(fork_id, vsock_path, readdress)| async move {
                    self.resume_vm(fork_id).await?;
                    tell_fork(&vsock_path, readdress.as_deref()).await?;
                    Ok::<(), VmmError>(())
                }),
        )
        .await
        .into_iter()
        .collect()
    }

    /// Snapshot `source_id` (and `disk` and `data`) into `dir` and load
    /// every fork, `configs`, from it, paused. Either all of them load, or
    /// none is left.
    async fn fork_from_snapshot(
        &self,
        source_id: &str,
        disk: Option<&SourceDisk>,
        data: &[DataDisk],
        configs: Vec<ProtoVmConfig>,
        dir: &Path,
        memory: &mut ForkMemory,
    ) -> Result<Vec<(String, FcVmInstance)>, VmmError> {
        let fork_ids: Vec<String> = configs.iter().map(|config| config.vm_id.clone()).collect();
        let disks = ForkDisks::new(disk, data, &configs);
        disks.write_back(source_id).await?;
        let (frozen, snapshot) = match self.snapshot_for_fork(source_id, &disks, dir, memory).await
        {
            Ok(taken) => taken,
            Err(e) => {
                // The snapshot may have reset the dirty-page log.
                memory.forget_base();
                return Err(e);
            }
        };
        let image = self.memory_image(source_id, snapshot, memory).await?;
        let devices = disks.finish(frozen, &fork_ids).await?;

        let loaded = join_all(configs.into_iter().zip(&devices).map(|(config, device)| {
            let disk = disk.zip(device.as_deref());
            self.load_fork(config, disk, data, dir, &image)
        }))
        .await;
        let mut forks = Vec::with_capacity(loaded.len());
        let mut first_error = None;
        for (fork_id, result) in fork_ids.iter().zip(loaded) {
            match result {
                Ok(instance) => forks.push((fork_id.clone(), instance)),
                Err(e) => {
                    warn!("FC: Fork {} of VM {} failed: {}", fork_id, source_id, e);
                    first_error.get_or_insert(e);
                }
            }
        }
        if let Some(e) = first_error {
            for (fork_id, instance) in forks {
                self.discard_fork(&fork_id, instance.process, &instance.tap_devices)
                    .await;
            }
            return Err(e);
        }
        Ok(forks)
    }

    /// Pause `source_id` if it runs, write a snapshot of it to `dir` (all of
    /// its memory, or the pages dirtied since `memory`'s base), freeze its
    /// `disks`, and resume it.
    async fn snapshot_for_fork(
        &self,
        source_id: &str,
        disks: &ForkDisks<'_>,
        dir: &Path,
        memory: &ForkMemory,
    ) -> Result<(Option<FrozenUpper>, MemorySnapshot), VmmError> {
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            VmmError::ProcessError(format!(
                "Failed to create snapshot directory {}: {}",
                dir.display(),
                e
            ))
        })?;

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(source_id)
            .ok_or_else(|| VmmError::VmNotFound(source_id.to_string()))?;
        let (micro_vm, state) = instance.live("VM is not running")?;
        let was_running = match *state {
            LiveState::Running => true,
            LiveState::Paused => false,
            LiveState::NotStarted | LiveState::Unknown => {
                return Err(VmmError::ProcessError("VM is not running".into()));
            }
        };
        if was_running {
            micro_vm
                .patch_vm(&Vm::new(FcVmState::Paused))
                .await
                .map_err(sdk_err)?;
            *state = LiveState::Paused;
        }
        let image = dir.join("mem.snap");
        let (mem_file, snapshot_type, taken) = match memory {
            ForkMemory::Tracked { base: Some(base) } => {
                let diff = dir.join("mem.diff");
                let taken = MemorySnapshot::Diff {
                    base: base.0.clone(),
                    diff: diff.clone(),
                    image,
                };
                (diff, SnapshotType::Diff, taken)
            }
            _ => (
                image.clone(),
                SnapshotType::Full,
                MemorySnapshot::Full(image),
            ),
        };
        let snapshot = async {
            micro_vm
                .create_snapshot(&SnapshotCreateParams {
                    mem_file_path: mem_file.display().to_string(),
                    snapshot_path: dir.join("vm.snap").display().to_string(),
                    snapshot_type: Some(snapshot_type),
                    // The forks read them on this host, from the page cache.
                    sync_snapshot_files: Some(false),
                })
                .await
                .map_err(sdk_err)?;
            disks.freeze(source_id).await
        }
        .await;
        if was_running {
            // Even when the snapshot failed: a fork mustn't leave its source paused.
            micro_vm
                .patch_vm(&Vm::new(FcVmState::Resumed))
                .await
                .map_err(sdk_err)?;
            *state = LiveState::Running;
        }
        snapshot.map(|frozen| (frozen, taken))
    }

    /// The full memory image of a fork round's `snapshot`, which its forks
    /// load. For a source whose dirty pages are tracked, it is the base of the
    /// next round too: the snapshot reset the dirty-page log.
    async fn memory_image(
        &self,
        source_id: &str,
        snapshot: MemorySnapshot,
        memory: &mut ForkMemory,
    ) -> Result<PathBuf, VmmError> {
        let image = match snapshot {
            MemorySnapshot::Full(image) => Ok(image),
            MemorySnapshot::Diff { base, diff, image } => {
                let dirty = tokio::fs::metadata(&diff)
                    .await
                    .map(|metadata| metadata.blocks() * 512 / (1024 * 1024))
                    .unwrap_or_default();
                info!("FC: Forking VM {source_id} from a diff snapshot, {dirty} MiB dirty");
                let merged = async {
                    let (from, to) = (base, image.clone());
                    blocking("copying the fork base", move || {
                        crate::storage::files::copy_sparse(&from, &to)
                    })
                    .await?;
                    let (from, to) = (diff.clone(), image.clone());
                    blocking("applying the diff snapshot", move || {
                        crate::storage::files::overlay_extents(&from, &to)
                    })
                    .await?;
                    Ok(image)
                }
                .await;
                if let Err(e) = tokio::fs::remove_file(&diff).await {
                    warn!(
                        "FC: Failed to remove diff snapshot {}: {}",
                        diff.display(),
                        e
                    );
                }
                merged
            }
        };
        let ForkMemory::Tracked { base } = memory else {
            return image;
        };
        // Whatever comes next, the old base no longer holds.
        *base = None;
        let image = image?;
        let kept = self
            .runtime_dir
            .join(format!("{source_id}.fork-base-{}", uuid::Uuid::new_v4()));
        tokio::fs::rename(&image, &kept).await.map_err(|e| {
            VmmError::ProcessError(format!(
                "Failed to keep {} as the fork base: {e}",
                image.display()
            ))
        })?;
        *base = Some(ForkBase(kept.clone()));
        Ok(kept)
    }

    /// Load one fork, `config`, paused from the snapshot in `dir`, onto TAPs
    /// of its own and with `disk`'s drive on the fork's own device.
    async fn load_fork(
        &self,
        mut config: ProtoVmConfig,
        disk: Option<(&SourceDisk, &str)>,
        data: &[DataDisk],
        dir: &Path,
        image: &Path,
    ) -> Result<FcVmInstance, VmmError> {
        let fork_id = config.vm_id.clone();
        let tracked = tracks_dirty_pages(&config);
        let vsock_path = self.vsock_socket_path(&fork_id);
        if let Some(vsock) = config.vsock.as_mut() {
            vsock.socket = Some(vsock_path.display().to_string());
        }
        // The drives to point at the fork's own files once it has loaded: the
        // snapshot names its source's.
        let mut drives = Vec::new();
        if let Some((disk, device)) = disk {
            for drive in config.disks.iter_mut().filter(|d| d.id == disk.drive_id) {
                drive.path = Some(device.to_string());
            }
            drives.push((disk.drive_id.clone(), device.to_string()));
        }
        for drive in config
            .disks
            .iter()
            .filter(|drive| data.iter().any(|data| data.drive_id == drive.id))
        {
            if let Some(path) = &drive.path {
                drives.push((drive.id.clone(), path.clone()));
            }
        }
        let tap_devices = Self::create_taps(&fork_id, &mut config.networks).await?;
        match give_fork_its_seed(&mut config, &self.cloud_init_seed_path(&fork_id)).await {
            Ok(Some(seed)) => drives.push((cloud_init::SEED_DRIVE.to_string(), seed)),
            Ok(None) => {}
            Err(e) => {
                self.discard_fork(&fork_id, None, &tap_devices).await;
                return Err(e);
            }
        }
        let network_overrides = config
            .networks
            .iter()
            .filter_map(|net| Some(NetworkOverride::new(net.id.clone(), net.tap.clone()?)))
            .collect();

        let process = match self.spawn_process(&fork_id).await {
            Ok(process) => process,
            Err(e) => {
                self.discard_fork(&fork_id, None, &tap_devices).await;
                return Err(e);
            }
        };
        let socket_path = self.socket_path(&fork_id);
        let loaded = async {
            let machine = Machine::attach(&socket_path).await.map_err(sdk_err)?;
            let micro_vm = machine
                .load_and_resume(&SnapshotLoadParams {
                    snapshot_path: dir.join("vm.snap").display().to_string(),
                    mem_backend: Some(Box::new(MemoryBackend::new(
                        BackendType::File,
                        image.display().to_string(),
                    ))),
                    track_dirty_pages: Some(tracked),
                    network_overrides: Some(network_overrides),
                    vsock_override: Some(Box::new(VsockOverride::new(
                        vsock_path.display().to_string(),
                    ))),
                    // Paused: the snapshot names the source's drives, and the
                    // fork's firewall comes before it runs.
                    resume_vm: Some(false),
                    ..Default::default()
                })
                .await
                .map_err(sdk_err)?;
            for (drive_id, path) in &drives {
                let mut drive = PartialDrive::new(drive_id.clone());
                drive.path_on_host = Some(path.clone());
                Self::api_request(
                    &socket_path,
                    "PATCH",
                    &format!("/drives/{drive_id}"),
                    &drive,
                )
                .await?;
            }
            Ok(micro_vm)
        }
        .await;

        match loaded {
            Ok(micro_vm) => Ok(FcVmInstance {
                proto_config: config,
                process: Some(process),
                socket_path,
                tap_devices,
                phase: FcPhase::Live {
                    micro_vm,
                    state: LiveState::Paused,
                    forking: ForkMemory::new(tracked),
                },
            }),
            Err(e) => {
                self.discard_fork(&fork_id, Some(process), &tap_devices)
                    .await;
                Err(e)
            }
        }
    }
    /// Stop a fork that won't be kept and remove its sockets, TAPs, disk and
    /// seed. Its log stays, as a deleted VM's does.
    async fn discard_fork(&self, fork_id: &str, process: Option<Child>, taps: &[String]) {
        if let Some(mut process) = process
            && let Err(e) = process.kill().await
        {
            warn!("FC: Failed to kill fork {}: {}", fork_id, e);
        }
        for tap in taps {
            Self::delete_tap_device(tap).await;
        }
        self.unmap_oci_disk(fork_id).await;
        for path in [
            self.socket_path(fork_id),
            self.vsock_socket_path(fork_id),
            self.cloud_init_seed_path(fork_id),
        ] {
            if let Err(e) = tokio::fs::remove_file(&path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                warn!("FC: Failed to remove {}: {}", path.display(), e);
            }
        }
    }
}
