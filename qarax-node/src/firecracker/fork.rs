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
//! Each round's image is a new file, whose page cache starts empty: a reflink
//! shares the base's blocks, not its pages in memory. A fork reading its memory
//! from there would wait on the disk for each page it touched. So the node
//! notes the pages a round's first fork has touched once its guest answers the
//! fork notice, and the next round reads those into its image's page cache as
//! soon as the image is made, while its forks load: they touch the same pages
//! first.
//!
//! A seed (see [`crate::vmm::seed`]) forks from a snapshot the node keeps
//! instead: its first fork as a seed takes a full snapshot of it, and its forks
//! from then on load that one, whose memory image they all map.
//!
//! The forks load paused and are resumed in a second step, so that their
//! firewall is in place before they run.

use std::ops::Range;
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
use crate::vmm::seed::{SeedDir, SeedSnapshot};
use crate::vmm::vms::Adding;

/// Whether Firecracker tracks the pages a VM dirties: for a VM with the guest
/// agent, which can be forked, so that its forks after the first take a diff
/// snapshot.
pub(super) fn tracks_dirty_pages(config: &ProtoVmConfig) -> bool {
    config.vsock.is_some()
}

/// A running VM's [`ForkMemory`], which a fork round holds from its snapshot
/// to its new base. Taken before the VM's own lock, never while holding it:
/// the round takes the VM's lock for its snapshot.
pub(super) type ForkMemoryLock = Arc<tokio::sync::Mutex<ForkMemory>>;

/// What the next fork of a running VM snapshots of its memory.
pub(super) enum ForkMemory {
    /// Firecracker doesn't track the pages the VM dirties, or qarax-node
    /// doesn't know that it does (a VM recovered at startup): each fork takes
    /// a full snapshot.
    Untracked,
    /// Firecracker tracks the pages the VM dirties. `base` is its memory as
    /// the last fork round's snapshot left it, the dirty-page log reset since:
    /// the next round writes only the pages dirtied since. `startup` is the
    /// parts of its memory (byte ranges) a fork had touched by the time its
    /// guest answered the fork notice, which the next round's forks touch
    /// first.
    Tracked {
        base: Option<ForkBase>,
        startup: Vec<Range<u64>>,
    },
    /// A seed: its forks as a seed load this snapshot of it
    /// ([`FirecrackerManager::fork_seed`]), and a fork of its current state
    /// takes a full snapshot, as an untracked VM's does.
    Seed(Arc<SeedSnapshot>),
}

impl ForkMemory {
    /// For a VM Firecracker runs with dirty-page tracking or without.
    pub(super) fn new(tracked: bool) -> ForkMemoryLock {
        Arc::new(tokio::sync::Mutex::new(if tracked {
            ForkMemory::Tracked {
                base: None,
                startup: Vec::new(),
            }
        } else {
            ForkMemory::Untracked
        }))
    }

    /// Forget the base, whose diffs no longer hold: something reset the
    /// dirty-page log.
    pub(super) fn forget_base(&mut self) {
        if let ForkMemory::Tracked { base, .. } = self {
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
    /// they make `image`, whose `startup` parts its forks touch first.
    Diff {
        base: PathBuf,
        diff: PathBuf,
        image: PathBuf,
        startup: Vec<Range<u64>>,
    },
}

impl FirecrackerManager {
    pub(super) async fn fork(&self, source_id: &str, forks: &[Fork]) -> Result<(), VmmError> {
        let (adding, source_config, memory) = self.begin_fork(source_id, forks).await?;
        // One fork round of a VM at a time: each diffs against the last one's
        // memory image.
        let mut memory = memory.lock().await;
        check_forkable(&source_config)?;
        let configs = forks
            .iter()
            .map(|fork| fork_config(&source_config, fork))
            .collect::<Result<Vec<_>, _>>()?;
        let data = data_disks(&source_config, forks)?;
        let disk = self.oci_disk(source_id, &source_config, &data).await?;

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
        self.add_forks(source_id, adding, forked?).await;
        Ok(())
    }

    /// Fork `seed_id`, a seed, into `forks`, paused: from the snapshot of it
    /// the node keeps, which its first fork as a seed takes.
    pub(super) async fn fork_seed(&self, seed_id: &str, forks: &[Fork]) -> Result<(), VmmError> {
        let (adding, seed_config, memory) = self.begin_fork(seed_id, forks).await?;
        let seed = {
            let mut memory = memory.lock().await;
            match &*memory {
                ForkMemory::Seed(seed) => seed.clone(),
                ForkMemory::Untracked | ForkMemory::Tracked { .. } => {
                    let taken = self.take_seed_snapshot(seed_id, seed_config, forks).await;
                    // Its full snapshot reset the dirty-page log, if it got so far.
                    memory.forget_base();
                    let seed = Arc::new(taken?);
                    *memory = ForkMemory::Seed(seed.clone());
                    seed
                }
            }
        };
        let configs = forks
            .iter()
            .map(|fork| fork_config(&seed.config, fork))
            .collect::<Result<Vec<_>, _>>()?;
        let devices = seed.give_disks(forks, &configs).await?;
        let snapshot = seed.vm();
        let forked = self
            .load_forks(
                seed_id,
                configs,
                seed.oci_disk(),
                &devices,
                seed.data(),
                &snapshot,
                &snapshot.join("mem.snap"),
            )
            .await?;
        self.add_forks(seed_id, adding, forked).await;
        Ok(())
    }

    /// Take a snapshot of `seed_id`, whose config is `config`, as a seed: all
    /// of its memory, its devices' state and its disks as they are, during a
    /// pause.
    async fn take_seed_snapshot(
        &self,
        seed_id: &str,
        config: ProtoVmConfig,
        forks: &[Fork],
    ) -> Result<SeedSnapshot, VmmError> {
        check_forkable(&config)?;
        let data = data_disks(&config, forks)?;
        let disk = self.oci_disk(seed_id, &config, &data).await?;
        let dir = SeedDir::new(&self.runtime_dir, seed_id).await?;
        let (disks, kept) = dir.disks(disk.as_ref(), &data);
        disks.write_back(seed_id).await?;
        let snapshot = dir.vm();
        let frozen = self
            .pause_for_snapshot(
                seed_id,
                &disks,
                &snapshot,
                &snapshot.join("mem.snap"),
                SnapshotType::Full,
            )
            .await?;
        info!("FC: Took the snapshot of VM {seed_id} its forks as a seed load");
        dir.keep(config, disk, frozen, kept).await
    }

    /// Start a fork round of `source_id` into `forks`: reserve their IDs, and
    /// take its config and [`ForkMemory`].
    async fn begin_fork(
        &self,
        source_id: &str,
        forks: &[Fork],
    ) -> Result<(Adding<'_, ManagedVm>, ProtoVmConfig, ForkMemoryLock), VmmError> {
        let fork_ids: Vec<String> = forks.iter().map(|fork| fork.vm_id.clone()).collect();
        info!("FC: Forking VM {} into {:?}", source_id, fork_ids);
        if fork_ids.is_empty() {
            return Err(VmmError::InvalidConfig("no VMs to fork into".into()));
        }
        let adding = self
            .vms
            .reserve(fork_ids)
            .map_err(VmmError::VmAlreadyExists)?;
        let instance = self.lock_vm(source_id).await?;
        let FcPhase::Live { forking, .. } = &instance.phase else {
            return Err(VmmError::ProcessError("VM is not running".into()));
        };
        Ok((adding, instance.proto_config.clone(), forking.clone()))
    }

    /// `source_id`'s OCI image disk, which its forks get a disk of their own
    /// on ([`source_disk`]).
    async fn oci_disk(
        &self,
        source_id: &str,
        config: &ProtoVmConfig,
        data: &[DataDisk],
    ) -> Result<Option<SourceDisk>, VmmError> {
        let oci = match &self.overlaybd {
            Some(backend) if backend.has_mapping(source_id).await => {
                Some(backend.manager().clone())
            }
            _ => None,
        };
        source_disk(source_id, config, data, oci)
    }

    /// Make `forks`, loaded from `source_id`, the manager's VMs.
    async fn add_forks(
        &self,
        source_id: &str,
        adding: Adding<'_, ManagedVm>,
        forks: Vec<(String, FcVmInstance)>,
    ) {
        for (fork_id, instance) in &forks {
            // Persist config for recovery, as create_vm does.
            if let Err(e) = tokio::fs::write(
                self.config_path(fork_id),
                instance.proto_config.encode_to_vec(),
            )
            .await
            {
                warn!("FC: Failed to persist config for VM {}: {}", fork_id, e);
            }
        }
        let count = forks.len();
        adding.finish(
            forks
                .into_iter()
                .map(|(fork_id, instance)| (fork_id, ManagedVm::new(instance))),
        );
        info!("FC: VM {} forked into {} VMs, paused", source_id, count);
    }

    /// Resume forks [`Self::fork`] loaded, and tell each one's guest that it
    /// is a fork and, if it has NICs, their addresses.
    pub(super) async fn resume_forks(
        &self,
        source_id: &str,
        fork_ids: &[String],
    ) -> Result<(), VmmError> {
        let config = |vm_id: &str| {
            self.vms
                .get(vm_id, |vm| vm.config.clone())
                .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))
        };
        let source = config(source_id)?;
        let forks = fork_ids
            .iter()
            .map(|fork_id| {
                Ok((
                    fork_id.as_str(),
                    self.vsock_socket_path(fork_id),
                    readdress_request(&source, &config(fork_id)?)?,
                ))
            })
            .collect::<Result<Vec<_>, VmmError>>()?;
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
        .collect::<Result<(), VmmError>>()?;
        if let Some(fork_id) = fork_ids.first() {
            self.note_startup(source_id, fork_id).await;
        }
        Ok(())
    }

    /// Note the parts of its memory that `fork_id`, a fork of `source_id`
    /// whose guest has just answered the fork notice, has touched, for the
    /// source's next fork round to read ahead ([`Self::memory_image`]). In the
    /// background, once the source's fork round in progress, if any, is over.
    async fn note_startup(&self, source_id: &str, fork_id: &str) {
        let pid = match self.lock_vm(fork_id).await {
            Ok(fork) => fork.process.as_ref().and_then(Child::id),
            Err(_) => None,
        };
        let source = self.vms.get(source_id, |vm| vm.instance.clone());
        let (Some(pid), Some(source)) = (pid, source) else {
            return;
        };
        let fork_id = fork_id.to_string();
        tokio::spawn(async move {
            let forking = match &*source.lock().await {
                Some(FcVmInstance {
                    phase: FcPhase::Live { forking, .. },
                    ..
                }) => forking.clone(),
                _ => return,
            };
            let mut memory = forking.lock().await;
            let ForkMemory::Tracked {
                base: Some(base),
                startup,
            } = &mut *memory
            else {
                return;
            };
            match crate::vmm::memory::mapped_pages(pid, &base.0).await {
                // None of a base a later round made: the fork maps the one before.
                Ok(pages) if pages.is_empty() => {}
                Ok(pages) => *startup = pages,
                Err(e) => warn!("FC: Failed to read the pages fork {fork_id} touched: {e}"),
            }
        });
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
        self.load_forks(source_id, configs, disk, &devices, data, dir, &image)
            .await
    }

    /// Load every fork, `configs`, of `source_id` paused from the snapshot in
    /// `dir` with the memory image `image`, each with `disk`'s drive on its own
    /// `devices` entry. Either all of them load, or none is left.
    #[allow(clippy::too_many_arguments)]
    async fn load_forks(
        &self,
        source_id: &str,
        configs: Vec<ProtoVmConfig>,
        disk: Option<&SourceDisk>,
        devices: &[Option<String>],
        data: &[DataDisk],
        dir: &Path,
        image: &Path,
    ) -> Result<Vec<(String, FcVmInstance)>, VmmError> {
        let fork_ids: Vec<String> = configs.iter().map(|config| config.vm_id.clone()).collect();
        let loaded = join_all(configs.into_iter().zip(devices).map(|(config, device)| {
            let disk = disk.zip(device.as_deref());
            self.load_fork(config, disk, data, dir, image)
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
        let image = dir.join("mem.snap");
        let (mem_file, snapshot_type, taken) = match memory {
            ForkMemory::Tracked {
                base: Some(base),
                startup,
            } => {
                let diff = dir.join("mem.diff");
                let taken = MemorySnapshot::Diff {
                    base: base.0.clone(),
                    diff: diff.clone(),
                    image,
                    startup: startup.clone(),
                };
                (diff, SnapshotType::Diff, taken)
            }
            _ => (
                image.clone(),
                SnapshotType::Full,
                MemorySnapshot::Full(image),
            ),
        };
        let frozen = self
            .pause_for_snapshot(source_id, disks, dir, &mem_file, snapshot_type)
            .await?;
        Ok((frozen, taken))
    }

    /// Pause `source_id` if it runs, write a snapshot of it to `dir`, its
    /// memory (all of it, or the pages dirtied since its last snapshot, as
    /// `snapshot_type` says) to `mem_file`, freeze its `disks`, and resume it.
    async fn pause_for_snapshot(
        &self,
        source_id: &str,
        disks: &ForkDisks<'_>,
        dir: &Path,
        mem_file: &Path,
        snapshot_type: SnapshotType,
    ) -> Result<Option<FrozenUpper>, VmmError> {
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            VmmError::ProcessError(format!(
                "Failed to create snapshot directory {}: {}",
                dir.display(),
                e
            ))
        })?;

        let mut instance = self.lock_vm(source_id).await?;
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
        snapshot
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
            MemorySnapshot::Diff {
                base,
                diff,
                image,
                startup,
            } => {
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
                if let Ok(image) = &merged {
                    read_ahead(image, startup).await;
                }
                merged
            }
        };
        let ForkMemory::Tracked { base, .. } = memory else {
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
        write_back_in_background(&kept);
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
            Ok(micro_vm) => Ok(FcVmInstance::new(
                config,
                Some(process),
                socket_path,
                tap_devices,
                FcPhase::Live {
                    micro_vm,
                    state: LiveState::Paused,
                    forking: ForkMemory::new(tracked),
                },
            )),
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

/// Start writing `base`, a fork base just made, back to disk, without
/// waiting. The next round's reflink of it has to wait for what is still dirty
/// then: all of the memory of a full snapshot, which Firecracker wrote to the
/// page cache only, so the sooner its write-back starts, the less is left.
fn write_back_in_background(base: &Path) {
    let base = base.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = crate::storage::files::write_back(&base) {
            warn!("FC: Failed to write back fork base {}: {e}", base.display());
        }
    });
}

/// Start reading `startup`'s parts of the memory image `image`, just made,
/// into its page cache, for the forks about to load it, without waiting: the
/// reads go on while they load.
async fn read_ahead(image: &Path, startup: Vec<Range<u64>>) {
    let file = match tokio::fs::File::open(image).await {
        Ok(file) => file.into_std().await,
        Err(e) => {
            warn!(
                "FC: Failed to open {} to read it ahead: {e}",
                image.display()
            );
            return;
        }
    };
    let image = image.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = crate::storage::files::read_ahead(&file, &startup) {
            warn!("FC: Failed to read ahead {}: {e}", image.display());
        }
    });
}
