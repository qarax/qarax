//! Forking: new microVMs that continue from a running one's current state,
//! its memory, devices and guest processes.
//!
//! The source is paused, snapshotted and resumed. Each fork is a new
//! Firecracker process that loads the snapshot with the `File` memory
//! backend, which maps it `MAP_PRIVATE`: the forks share the source's memory
//! pages until they write them. A fork's vsock device gets the fork's own
//! socket; the guest keeps the source's CID, which only it sees.
//!
//! A fork resumes with its source's random generator state and clock, so the
//! node tells each fork's guest agent that it was forked, with fresh entropy
//! and the time: qarax-init reseeds the kernel's generator and sets the clock.
//!
//! A source booted from an OCI image forks its disk too. Its OverlayBD
//! writable layer is copied during the pause (a reflink where the filesystem
//! has them) and sealed into a read-only layer once the source runs again.
//! Each fork's disk stacks a writable layer of its own on the source's layers
//! and that one, and the fork's drive is pointed at it before it resumes.
//!
//! A fork's NICs are its own: the control plane gives each an address on its
//! source NIC's network, and the fork loads the snapshot onto TAPs of its own
//! (`network_overrides`). Its guest resumes with the source's MAC and IP
//! addresses, which the fork's firewall keeps off the network, until the node
//! hands its guest agent the fork's after the fork notice.
//!
//! A source with cloud-init forks its seed image: each fork gets one of its
//! own with the same content, which outlives its source's.
//!
//! The forks load paused and are resumed in a second step, so that their
//! firewall is in place before they run. Only VMs with no disk but their OCI
//! image's and their cloud-init seed fork: a fork would write to its source's
//! other disk files.

use std::io::Read;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use firecracker_rust_sdk::models::{NetworkOverride, PartialDrive, VsockOverride};
use futures::future::join_all;

use super::*;
use crate::overlaybd::{FrozenUpper, OverlayBdManager};

/// How long a fork's guest agent has to answer the fork notice once it resumes.
const FORK_NOTICE_TIMEOUT: Duration = Duration::from_secs(10);

/// Bytes of entropy each fork gets.
const FORK_ENTROPY_BYTES: usize = 64;

/// Refuse a VM that has no guest agent to tell its forks they are forks. Its
/// NICs are [`fork_config`]'s, its disks [`FirecrackerManager::source_disk`]'s.
fn check_forkable(config: &ProtoVmConfig) -> Result<(), VmmError> {
    if config.vsock.is_none() {
        return Err(VmmError::InvalidConfig(
            "can't fork a VM without the guest agent, which reseeds each fork's random generator"
                .into(),
        ));
    }
    Ok(())
}

/// `fork`'s config: its source's under its ID, with its own NICs, one for each
/// of the source's (by id), each with the addresses its guest is to take.
fn fork_config(source: &ProtoVmConfig, fork: &Fork) -> Result<ProtoVmConfig, VmmError> {
    let ids = |networks: &[ProtoNetConfig]| {
        let mut ids: Vec<String> = networks.iter().map(|net| net.id.clone()).collect();
        ids.sort();
        ids
    };
    if ids(&fork.networks) != ids(&source.networks) {
        return Err(VmmError::InvalidConfig(format!(
            "fork {} needs a NIC for each of its source's, {:?}",
            fork.vm_id,
            ids(&source.networks)
        )));
    }
    if let Some(net) = fork
        .networks
        .iter()
        .find(|net| net.mac.is_none() || net.ip.is_none() || net.mask.is_none())
    {
        return Err(VmmError::InvalidConfig(format!(
            "NIC {} of fork {} has no MAC or IP address for its guest",
            net.id, fork.vm_id
        )));
    }
    Ok(ProtoVmConfig {
        vm_id: fork.vm_id.clone(),
        networks: fork.networks.clone(),
        ..source.clone()
    })
}

/// What gives a fork's guest its NICs' addresses, on one line:
/// `{"readdress": [{"from": MAC, "mac": MAC, "ip": IP, "mask": MASK}]}`, each
/// NIC by the source's MAC, which the guest still has. None for a fork without
/// NICs, whose guest may run a qarax-init older than the request.
fn readdress_request(
    source: &ProtoVmConfig,
    fork: &ProtoVmConfig,
) -> Result<Option<Vec<u8>>, VmmError> {
    if fork.networks.is_empty() {
        return Ok(None);
    }
    let nics = fork
        .networks
        .iter()
        .map(|net| {
            let from = source
                .networks
                .iter()
                .find(|source_net| source_net.id == net.id)
                .and_then(|source_net| source_net.mac.clone())
                .ok_or_else(|| {
                    VmmError::InvalidConfig(format!(
                        "the source of fork {} has no MAC for NIC {}",
                        fork.vm_id, net.id
                    ))
                })?;
            Ok(serde_json::json!({
                "from": from, "mac": net.mac, "ip": net.ip, "mask": net.mask
            }))
        })
        .collect::<Result<Vec<_>, VmmError>>()?;
    let mut request =
        serde_json::to_vec(&serde_json::json!({ "readdress": nics })).map_err(|e| {
            VmmError::ProcessError(format!("failed to encode the fork's addresses: {e}"))
        })?;
    request.push(b'\n');
    Ok(Some(request))
}

/// The fork notice qarax-init reads, on one line:
/// `{"forked": {"entropy": HEX, "unix_time_ns": N}}`.
fn fork_notice() -> Result<Vec<u8>, VmmError> {
    let mut entropy = [0u8; FORK_ENTROPY_BYTES];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut urandom| urandom.read_exact(&mut entropy))
        .map_err(|e| VmmError::ProcessError(format!("failed to read /dev/urandom: {e}")))?;
    let unix_time_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_nanos()).ok())
        .ok_or_else(|| VmmError::ProcessError("the system clock is out of range".into()))?;
    let entropy: String = entropy.iter().map(|b| format!("{b:02x}")).collect();
    let mut notice = serde_json::to_vec(&serde_json::json!({
        "forked": { "entropy": entropy, "unix_time_ns": unix_time_ns }
    }))
    .map_err(|e| VmmError::ProcessError(format!("failed to encode the fork notice: {e}")))?;
    notice.push(b'\n');
    Ok(notice)
}

/// A source's OCI image disk, which its forks get a copy of.
struct SourceDisk {
    /// Its Firecracker drive.
    drive_id: String,
    /// Its OverlayBD device, as Firecracker opens it.
    device: String,
    overlaybd: Arc<OverlayBdManager>,
}

fn overlaybd_err(e: crate::overlaybd::OverlayBdError) -> VmmError {
    VmmError::OverlayBdError(e.to_string())
}

impl FirecrackerManager {
    /// The disk `source_id`'s forks get a copy of, if it has one: its OCI
    /// image's, the only disk OverlayBD maps for a VM. Its cloud-init seed is
    /// each fork's own ([`Self::give_fork_its_seed`]); any other disk is refused.
    async fn source_disk(
        &self,
        source_id: &str,
        config: &ProtoVmConfig,
    ) -> Result<Option<SourceDisk>, VmmError> {
        let seed = cloud_init_seed(config).is_some();
        let disks: Vec<_> = config
            .disks
            .iter()
            .filter(|disk| !(seed && disk.id == CLOUD_INIT_DRIVE))
            .collect();
        let disk = match disks.as_slice() {
            [] => return Ok(None),
            [disk] => *disk,
            _ => None.ok_or_else(|| {
                VmmError::InvalidConfig(
                    "can't fork a VM with disks besides its OCI image's: its forks would write \
                     to the same files"
                        .into(),
                )
            })?,
        };
        let backend = match &self.overlaybd {
            Some(backend) if backend.has_mapping(source_id).await => backend,
            _ => {
                return Err(VmmError::InvalidConfig(
                    "can't fork a VM whose disk isn't its OCI image's: its forks would write to \
                     the same file"
                        .into(),
                ));
            }
        };
        let device = disk.path.clone().ok_or_else(|| {
            VmmError::InvalidConfig(format!("disk {} of VM {source_id} has no path", disk.id))
        })?;
        Ok(Some(SourceDisk {
            drive_id: disk.id.clone(),
            device,
            overlaybd: backend.manager().clone(),
        }))
    }

    pub(super) async fn fork(&self, source_id: &str, forks: &[Fork]) -> Result<(), VmmError> {
        let fork_ids: Vec<String> = forks.iter().map(|fork| fork.vm_id.clone()).collect();
        info!("FC: Forking VM {} into {:?}", source_id, fork_ids);
        if fork_ids.is_empty() {
            return Err(VmmError::InvalidConfig("no VMs to fork into".into()));
        }
        let source_config = {
            let vms = self.vms.lock().await;
            if let Some(id) = fork_ids.iter().find(|id| vms.contains_key(*id)) {
                return Err(VmmError::VmAlreadyExists(id.clone()));
            }
            vms.get(source_id)
                .ok_or_else(|| VmmError::VmNotFound(source_id.to_string()))?
                .proto_config
                .clone()
        };
        check_forkable(&source_config)?;
        let configs = forks
            .iter()
            .map(|fork| fork_config(&source_config, fork))
            .collect::<Result<Vec<_>, _>>()?;
        let disk = self.source_disk(source_id, &source_config).await?;

        let snapshot_dir = self
            .runtime_dir
            .join(format!("{source_id}.fork-{}", uuid::Uuid::new_v4()));
        let forked = self
            .fork_from_snapshot(source_id, disk.as_ref(), configs, &snapshot_dir)
            .await;
        // The forks keep the memory file mapped, so it can go now.
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
                    Self::send_fork_notice(&vsock_path).await?;
                    if let Some(readdress) = readdress {
                        Self::readdress_guest(&vsock_path, &readdress).await?;
                    }
                    Ok::<(), VmmError>(())
                }),
        )
        .await
        .into_iter()
        .collect()
    }

    /// Snapshot `source_id` (and `disk`) into `dir` and load every fork,
    /// `configs`, from it, paused. Either all of them load, or none is left.
    async fn fork_from_snapshot(
        &self,
        source_id: &str,
        disk: Option<&SourceDisk>,
        configs: Vec<ProtoVmConfig>,
        dir: &Path,
    ) -> Result<Vec<(String, FcVmInstance)>, VmmError> {
        let fork_ids: Vec<String> = configs.iter().map(|config| config.vm_id.clone()).collect();
        if let Some(disk) = disk {
            // While the source still runs, so the pause only writes back the rest.
            disk.overlaybd
                .write_back_upper(source_id)
                .await
                .map_err(overlaybd_err)?;
        }
        let frozen = self.snapshot_for_fork(source_id, disk, dir).await?;
        let devices = match (disk, frozen) {
            (Some(disk), Some(frozen)) => {
                let devices = Self::mount_fork_disks(&disk.overlaybd, &frozen, &fork_ids).await;
                // The forks that got a disk have their own link to the layer.
                disk.overlaybd.release(frozen).await;
                devices?.into_iter().map(Some).collect()
            }
            _ => vec![None; fork_ids.len()],
        };

        let loaded = join_all(configs.into_iter().zip(&devices).map(|(config, device)| {
            let disk = disk.zip(device.as_deref());
            self.load_fork(config, disk, dir)
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

    /// Seal the frozen layer and mount a disk for each fork on it, one at a
    /// time (finding a new TCMU device is serial anyway). None is left mounted
    /// if one fails.
    async fn mount_fork_disks(
        overlaybd: &OverlayBdManager,
        frozen: &FrozenUpper,
        fork_ids: &[String],
    ) -> Result<Vec<String>, VmmError> {
        overlaybd.seal(frozen).await.map_err(overlaybd_err)?;
        let mut devices = Vec::with_capacity(fork_ids.len());
        for fork_id in fork_ids {
            match overlaybd.mount_fork(fork_id, frozen).await {
                Ok(mounted) => devices.push(mounted.device_path),
                Err(e) => {
                    for mounted_id in &fork_ids[..=devices.len()] {
                        overlaybd.unmount(mounted_id).await;
                    }
                    return Err(overlaybd_err(e));
                }
            }
        }
        Ok(devices)
    }

    /// Pause `source_id` if it runs, write a full snapshot of it to `dir`,
    /// freeze `disk`'s writable layer, and resume it.
    async fn snapshot_for_fork(
        &self,
        source_id: &str,
        disk: Option<&SourceDisk>,
        dir: &Path,
    ) -> Result<Option<FrozenUpper>, VmmError> {
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
        let snapshot = async {
            micro_vm
                .create_snapshot(&SnapshotCreateParams::new(
                    dir.join("mem.snap").display().to_string(),
                    dir.join("vm.snap").display().to_string(),
                ))
                .await
                .map_err(sdk_err)?;
            match disk {
                Some(disk) => disk
                    .overlaybd
                    .freeze_upper(source_id, &disk.device)
                    .await
                    .map(Some)
                    .map_err(overlaybd_err),
                None => Ok(None),
            }
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

    /// Load one fork, `config`, paused from the snapshot in `dir`, onto TAPs
    /// of its own and with `disk`'s drive on the fork's own device.
    async fn load_fork(
        &self,
        mut config: ProtoVmConfig,
        disk: Option<(&SourceDisk, &str)>,
        dir: &Path,
    ) -> Result<FcVmInstance, VmmError> {
        let fork_id = config.vm_id.clone();
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
        let tap_devices = Self::create_taps(&fork_id, &mut config.networks).await?;
        match self.give_fork_its_seed(&mut config).await {
            Ok(Some(seed)) => drives.push((CLOUD_INIT_DRIVE.to_string(), seed)),
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
                        dir.join("mem.snap").display().to_string(),
                    ))),
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
                },
            }),
            Err(e) => {
                self.discard_fork(&fork_id, Some(process), &tap_devices)
                    .await;
                Err(e)
            }
        }
    }

    /// Give a fork, `config`, a cloud-init seed image of its own if its source
    /// has one: the same content at the fork's path, pointed to by its drive.
    /// Returns its path.
    async fn give_fork_its_seed(
        &self,
        config: &mut ProtoVmConfig,
    ) -> Result<Option<String>, VmmError> {
        let Some(ci) = cloud_init_seed(config).cloned() else {
            return Ok(None);
        };
        let seed = self
            .write_cloud_init_seed(&config.vm_id, &ci)
            .await?
            .display()
            .to_string();
        for drive in config.disks.iter_mut().filter(|d| d.id == CLOUD_INIT_DRIVE) {
            drive.path = Some(seed.clone());
        }
        Ok(Some(seed))
    }

    /// Give a fork's guest its NICs' addresses ([`readdress_request`]).
    async fn readdress_guest(vsock_path: &Path, request: &[u8]) -> Result<(), VmmError> {
        const TIMEOUT: Duration = Duration::from_secs(10);

        let response = guest_agent::request(vsock_path, request, TIMEOUT).await?;
        let ack: guest_agent::Ack = serde_json::from_slice(&response).map_err(|e| {
            VmmError::ExecError(format!(
                "invalid answer to the fork's addresses from the guest agent: {e}"
            ))
        })?;
        if ack.exit_code != 0 {
            // An older qarax-init answers it as an invalid exec request.
            return Err(VmmError::ExecError(format!(
                "the guest agent did not take the fork's addresses (exit {}): {}",
                ack.exit_code,
                ack.stderr.trim_end()
            )));
        }
        Ok(())
    }

    /// Tell a fork's guest that it was forked. Its agent was listening when
    /// the snapshot was taken, and answers on the new socket once the guest
    /// has handled the vsock reset that comes with a restore; until then,
    /// connecting fails, so retry until the deadline.
    async fn send_fork_notice(vsock_path: &Path) -> Result<(), VmmError> {
        const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(2);
        const RETRY_INTERVAL: Duration = Duration::from_millis(5);

        let deadline = Instant::now() + FORK_NOTICE_TIMEOUT;
        loop {
            // A fresh notice per attempt, so the time it carries is current.
            let notice = fork_notice()?;
            match guest_agent::request(vsock_path, &notice, ATTEMPT_TIMEOUT).await {
                Ok(response) => {
                    let ack: guest_agent::Ack = serde_json::from_slice(&response).map_err(|e| {
                        VmmError::ExecError(format!(
                            "invalid answer to the fork notice from the guest agent: {e}"
                        ))
                    })?;
                    if ack.exit_code != 0 {
                        // An older qarax-init answers it as an invalid exec request.
                        return Err(VmmError::ExecError(format!(
                            "the guest agent refused the fork notice (exit {}): its \
                             qarax-init may be older than forking",
                            ack.exit_code
                        )));
                    }
                    return Ok(());
                }
                Err(e) if Instant::now() < deadline => {
                    debug!("FC: Fork notice not delivered yet: {}", e);
                    tokio::time::sleep(RETRY_INTERVAL).await;
                }
                Err(e) => return Err(e),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::node::{CloudInitConfig, DiskConfig, NetConfig, VsockConfig};

    fn with_guest_agent() -> ProtoVmConfig {
        ProtoVmConfig {
            vm_id: "source".into(),
            vsock: Some(VsockConfig::default()),
            ..Default::default()
        }
    }

    #[test]
    fn a_vm_with_the_guest_agent_and_nothing_to_share_forks() {
        assert!(check_forkable(&with_guest_agent()).is_ok());
    }

    #[test]
    fn a_vm_without_the_guest_agent_does_not_fork() {
        let config = ProtoVmConfig {
            vsock: None,
            ..with_guest_agent()
        };
        assert!(matches!(
            check_forkable(&config),
            Err(VmmError::InvalidConfig(_))
        ));
    }

    fn nic(id: &str, mac: &str, ip: &str) -> NetConfig {
        NetConfig {
            id: id.into(),
            mac: Some(mac.into()),
            ip: Some(ip.into()),
            mask: Some("255.255.255.0".into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_fork_has_a_nic_of_its_own_for_each_of_its_sources() {
        let source = ProtoVmConfig {
            networks: vec![
                nic("net0", "02:00:00:00:00:01", "10.0.0.2"),
                nic("net1", "02:00:00:00:01:01", "10.0.1.2"),
            ],
            ..with_guest_agent()
        };
        let fork = |networks| Fork {
            vm_id: "fork".into(),
            networks,
            firewall: vec![],
        };
        let own = vec![
            nic("net1", "02:00:00:00:01:02", "10.0.1.3"),
            nic("net0", "02:00:00:00:00:02", "10.0.0.3"),
        ];

        let config = fork_config(&source, &fork(own.clone())).unwrap();
        assert_eq!(config.vm_id, "fork");
        assert_eq!(config.networks, own);

        let missing = vec![own[0].clone()];
        let no_address = vec![
            own[0].clone(),
            NetConfig {
                ip: None,
                ..own[1].clone()
            },
        ];
        for networks in [missing, no_address] {
            assert!(matches!(
                fork_config(&source, &fork(networks)),
                Err(VmmError::InvalidConfig(_))
            ));
        }
    }

    #[test]
    fn a_forks_guest_finds_its_nics_by_its_sources_macs() {
        let source = ProtoVmConfig {
            networks: vec![nic("net0", "02:00:00:00:00:01", "10.0.0.2")],
            ..with_guest_agent()
        };
        let fork = ProtoVmConfig {
            networks: vec![nic("net0", "02:00:00:00:00:02", "10.0.0.3")],
            ..with_guest_agent()
        };

        let request = readdress_request(&source, &fork).unwrap().unwrap();
        assert_eq!(request.last(), Some(&b'\n'));
        let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
        assert_eq!(
            request,
            serde_json::json!({"readdress": [{
                "from": "02:00:00:00:00:01",
                "mac": "02:00:00:00:00:02",
                "ip": "10.0.0.3",
                "mask": "255.255.255.0",
            }]})
        );

        // Nothing to tell a fork without NICs.
        assert!(
            readdress_request(&with_guest_agent(), &with_guest_agent())
                .unwrap()
                .is_none()
        );
    }

    fn disk(id: &str) -> DiskConfig {
        DiskConfig {
            id: id.into(),
            path: Some(format!("/dev/{id}")),
            ..Default::default()
        }
    }

    /// A manager on a node with OverlayBD, where `mapped` has an OverlayBD disk.
    async fn manager_with_overlaybd(dir: &tempfile::TempDir, mapped: &str) -> FirecrackerManager {
        let cache = dir.path().join("overlaybd");
        tokio::fs::create_dir_all(cache.join(mapped)).await.unwrap();
        tokio::fs::write(cache.join(mapped).join("config.json"), "{}")
            .await
            .unwrap();
        let overlaybd = Arc::new(OverlayBdManager::new("unused-convertor", &cache));
        FirecrackerManager::new(
            dir.path(),
            "/bin/true",
            Duration::from_secs(1),
            Some(Arc::new(OverlayBdBackend::new(overlaybd, None))),
        )
    }

    #[tokio::test]
    async fn a_fork_gets_a_cloud_init_seed_of_its_own() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager =
            FirecrackerManager::new(dir.path(), "/bin/true", Duration::from_secs(1), None);
        let ci = CloudInitConfig {
            user_data: "#cloud-config\n".into(),
            meta_data: "instance-id: source\n".into(),
            ..Default::default()
        };
        let source_seed = manager.cloud_init_seed_path("source");
        let mut config = ProtoVmConfig {
            vm_id: "fork".into(),
            cloud_init: Some(ci.clone()),
            disks: vec![DiskConfig {
                id: CLOUD_INIT_DRIVE.into(),
                path: Some(source_seed.display().to_string()),
                readonly: Some(true),
                ..Default::default()
            }],
            ..with_guest_agent()
        };

        let seed = manager
            .give_fork_its_seed(&mut config)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            seed,
            manager.cloud_init_seed_path("fork").display().to_string()
        );
        assert_eq!(config.disks[0].path.as_deref(), Some(seed.as_str()));
        assert_eq!(
            std::fs::read(&seed).unwrap(),
            crate::cloud_init::build_seed_image(&ci.user_data, &ci.meta_data, None).unwrap()
        );

        // Without cloud-init, nothing.
        let mut without = with_guest_agent();
        assert!(
            manager
                .give_fork_its_seed(&mut without)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_fork_gets_its_sources_oci_disk_and_no_other() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = manager_with_overlaybd(&dir, "source").await;
        let with_disks = |disks| ProtoVmConfig {
            disks,
            ..with_guest_agent()
        };

        let none = manager.source_disk("source", &with_disks(vec![])).await;
        assert!(matches!(none, Ok(None)));

        let oci = manager
            .source_disk("source", &with_disks(vec![disk("rootfs")]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (oci.drive_id.as_str(), oci.device.as_str()),
            ("rootfs", "/dev/rootfs")
        );

        // Its cloud-init seed is each fork's own.
        let with_seed = ProtoVmConfig {
            cloud_init: Some(CloudInitConfig {
                user_data: "#cloud-config\n".into(),
                ..Default::default()
            }),
            ..with_disks(vec![disk("rootfs"), disk(CLOUD_INIT_DRIVE)])
        };
        let oci = manager
            .source_disk("source", &with_seed)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(oci.drive_id, "rootfs");

        // A disk OverlayBD didn't map for the VM, or a second disk.
        for (vm, disks) in [
            ("unmapped", vec![disk("data")]),
            ("source", vec![disk("rootfs"), disk("data")]),
            ("source", vec![disk("rootfs"), disk(CLOUD_INIT_DRIVE)]),
        ] {
            assert!(matches!(
                manager.source_disk(vm, &with_disks(disks)).await,
                Err(VmmError::InvalidConfig(_))
            ));
        }

        let without_overlaybd =
            FirecrackerManager::new(dir.path(), "/bin/true", Duration::from_secs(1), None);
        assert!(matches!(
            without_overlaybd
                .source_disk("source", &with_disks(vec![disk("rootfs")]))
                .await,
            Err(VmmError::InvalidConfig(_))
        ));
    }
}
