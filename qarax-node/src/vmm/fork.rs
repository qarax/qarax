//! Forking a running VM, whatever its hypervisor: new VMs that continue from
//! its current state, its memory, devices and guest processes. What doesn't
//! depend on the hypervisor is here; each backend snapshots its source,
//! between [`ForkDisks::write_back`] and [`ForkDisks::finish`], and loads the
//! forks.
//!
//! A fork resumes with its source's random generator state and clock, so the
//! node tells each fork's guest agent that it was forked, with fresh entropy
//! and the time: qarax-init reseeds the kernel's generator and sets the clock.
//!
//! A fork's NICs are its own: the control plane gives each an address on its
//! source NIC's network, and the fork comes up on TAPs of its own. Its guest
//! resumes with the source's MAC and IP addresses, which the fork's firewall
//! keeps off the network, until the node hands its guest agent the fork's after
//! the fork notice.
//!
//! A source booted from an OCI image forks its disk too. Its OverlayBD writable
//! layer is copied during the pause (a reflink where the filesystem has them)
//! and sealed into a read-only layer once the source runs again. Each fork's
//! disk stacks a writable layer of its own on the source's layers and that one.
//!
//! Each of a source's data disks is copied for its forks: during the pause into
//! the first fork's file (a reflink where the filesystem has them, its data
//! extents only elsewhere), and from that into the others' once the source runs
//! again. Read-only disks are shared, and the cloud-init seed image is each
//! fork's own, with the same content.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tracing::debug;

use super::{VmmError, guest_agent};
use crate::cloud_init;
use crate::overlaybd::{FrozenUpper, OverlayBdManager};
use crate::rpc::node::{Fork, NetConfig as ProtoNetConfig, VmConfig as ProtoVmConfig};

/// How long a fork's guest agent has to answer the fork notice once it resumes.
const FORK_NOTICE_TIMEOUT: Duration = Duration::from_secs(10);

/// Bytes of entropy each fork gets.
const FORK_ENTROPY_BYTES: usize = 64;

/// Refuse a VM that has no guest agent to tell its forks they are forks. Its
/// NICs are [`fork_config`]'s, its disks [`FirecrackerManager::source_disk`]'s.
pub(crate) fn check_forkable(config: &ProtoVmConfig) -> Result<(), VmmError> {
    if config.vsock.is_none() {
        return Err(VmmError::InvalidConfig(
            "can't fork a VM without the guest agent, which reseeds each fork's random generator"
                .into(),
        ));
    }
    Ok(())
}

/// A source's data disk, of which each fork gets a copy.
pub(crate) struct DataDisk {
    pub(crate) drive_id: String,
    pub(crate) path: PathBuf,
}

/// The source's data disks: the drives the forks copy, each fork the same.
pub(crate) fn data_disks(
    source: &ProtoVmConfig,
    forks: &[Fork],
) -> Result<Vec<DataDisk>, VmmError> {
    fn copied(fork: &Fork) -> Vec<&str> {
        let mut ids: Vec<&str> = fork
            .disks
            .iter()
            .map(|copy| copy.drive_id.as_str())
            .collect();
        ids.sort();
        ids
    }
    let ids = forks.first().map(copied).unwrap_or_default();
    if let Some(fork) = forks.iter().find(|fork| copied(fork) != ids) {
        return Err(VmmError::InvalidConfig(format!(
            "fork {} copies other disks than its siblings, {ids:?}",
            fork.vm_id
        )));
    }
    ids.into_iter()
        .map(|id| {
            let path = source
                .disks
                .iter()
                .find(|disk| disk.id == id)
                .and_then(|disk| disk.path.clone())
                .ok_or_else(|| {
                    VmmError::InvalidConfig(format!("the source has no disk file {id} to copy"))
                })?;
            Ok(DataDisk {
                drive_id: id.to_string(),
                path: path.into(),
            })
        })
        .collect()
}

/// `fork`'s config: its source's under its ID, with its own NICs, one for each
/// of the source's (by id), each with the addresses its guest is to take, and
/// its copies of the source's data disks.
pub(crate) fn fork_config(source: &ProtoVmConfig, fork: &Fork) -> Result<ProtoVmConfig, VmmError> {
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
    let mut config = ProtoVmConfig {
        vm_id: fork.vm_id.clone(),
        networks: fork.networks.clone(),
        ..source.clone()
    };
    for copy in &fork.disks {
        let drive = config
            .disks
            .iter_mut()
            .find(|disk| disk.id == copy.drive_id)
            .ok_or_else(|| {
                VmmError::InvalidConfig(format!(
                    "fork {} copies disk {}, which its source doesn't have",
                    fork.vm_id, copy.drive_id
                ))
            })?;
        drive.path = Some(copy.path.clone());
    }
    Ok(config)
}

/// What gives a fork's guest its NICs' addresses, on one line:
/// `{"readdress": [{"from": MAC, "mac": MAC, "ip": IP, "mask": MASK}]}`, each
/// NIC by the source's MAC, which the guest still has. None for a fork without
/// NICs, whose guest may run a qarax-init older than the request.
pub(crate) fn readdress_request(
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

/// Run `op`, a blocking file operation (`what`), off the async runtime.
pub(crate) async fn blocking(
    what: &str,
    op: impl FnOnce() -> std::io::Result<()> + Send + 'static,
) -> Result<(), VmmError> {
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|e| VmmError::StorageError(format!("{what}: {e}")))?
        .map_err(|e| VmmError::StorageError(format!("{what}: {e}")))
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
pub(crate) struct SourceDisk {
    /// Its drive.
    pub(crate) drive_id: String,
    /// Its OverlayBD device, as the VMM opens it.
    pub(crate) device: String,
    pub(crate) overlaybd: Arc<OverlayBdManager>,
}

pub(crate) fn overlaybd_err(e: crate::overlaybd::OverlayBdError) -> VmmError {
    VmmError::OverlayBdError(e.to_string())
}

/// The OCI image disk `source_id`'s forks get, if it has one: the only disk
/// OverlayBD maps for a VM, `oci` the manager if it maps one for the VM. Its
/// cloud-init seed is each fork's own ([`give_fork_its_seed`]), its `data`
/// disks are copied, and its read-only disks shared; any other disk is
/// refused.
pub(crate) fn source_disk(
    source_id: &str,
    config: &ProtoVmConfig,
    data: &[DataDisk],
    oci: Option<Arc<OverlayBdManager>>,
) -> Result<Option<SourceDisk>, VmmError> {
    let seed = cloud_init::seed_config(config).is_some();
    let disks: Vec<_> = config
        .disks
        .iter()
        .filter(|disk| !(seed && disk.id == cloud_init::SEED_DRIVE))
        .filter(|disk| !disk.readonly.unwrap_or(false))
        .filter(|disk| !data.iter().any(|data| data.drive_id == disk.id))
        .collect();
    let disk = match disks.as_slice() {
        [] => return Ok(None),
        [disk] => *disk,
        _ => {
            return Err(VmmError::InvalidConfig(
                "can't fork a VM with disks besides its OCI image's: its forks would write \
                 to the same files"
                    .into(),
            ));
        }
    };
    let overlaybd = oci.ok_or_else(|| {
        VmmError::InvalidConfig(
            "can't fork a VM whose disk isn't its OCI image's: its forks would write to the \
             same file"
                .into(),
        )
    })?;
    let device = disk.path.clone().ok_or_else(|| {
        VmmError::InvalidConfig(format!("disk {} of VM {source_id} has no path", disk.id))
    })?;
    Ok(Some(SourceDisk {
        drive_id: disk.id.clone(),
        device,
        overlaybd,
    }))
}

/// The disks a fork round forks: its source's OCI image disk, and its data
/// disks with each fork's copies.
pub(crate) struct ForkDisks<'a> {
    pub(crate) oci: Option<&'a SourceDisk>,
    /// Each data disk's file, and its copies, one for each fork in order.
    copies: Vec<(PathBuf, Vec<PathBuf>)>,
}

impl<'a> ForkDisks<'a> {
    pub(crate) fn new(
        oci: Option<&'a SourceDisk>,
        data: &[DataDisk],
        configs: &[ProtoVmConfig],
    ) -> Self {
        let copies = data
            .iter()
            .map(|disk| {
                let copies = configs
                    .iter()
                    .filter_map(|config| {
                        config
                            .disks
                            .iter()
                            .find(|drive| drive.id == disk.drive_id)?
                            .path
                            .as_ref()
                            .map(PathBuf::from)
                    })
                    .collect();
                (disk.path.clone(), copies)
            })
            .collect();
        ForkDisks { oci, copies }
    }

    /// While the source still runs: write back what it wrote to its disks, so
    /// that the pause only writes back the rest.
    pub(crate) async fn write_back(&self, source_id: &str) -> Result<(), VmmError> {
        if let Some(oci) = self.oci {
            oci.overlaybd
                .write_back_upper(source_id)
                .await
                .map_err(overlaybd_err)?;
        }
        for (file, _) in &self.copies {
            let file = file.clone();
            blocking("writing back a data disk", move || {
                crate::storage::files::write_back(&file)
            })
            .await?;
        }
        Ok(())
    }

    /// In the pause, after the memory snapshot: copy each data disk into the
    /// first fork's file, and freeze the OCI disk's writable layer.
    pub(crate) async fn freeze(&self, source_id: &str) -> Result<Option<FrozenUpper>, VmmError> {
        for (file, copies) in &self.copies {
            if let Some(first) = copies.first() {
                let (file, first) = (file.clone(), first.clone());
                blocking("copying a data disk", move || {
                    crate::storage::files::copy_sparse(&file, &first)
                })
                .await?;
            }
        }
        match self.oci {
            Some(oci) => oci
                .overlaybd
                .freeze_upper(source_id, &oci.device)
                .await
                .map(Some)
                .map_err(overlaybd_err),
            None => Ok(None),
        }
    }

    /// Once the source runs again: the other forks' copies of the data disks,
    /// and each fork's OCI disk on `frozen`, the layer [`Self::freeze`] made.
    /// Returns each fork's OCI disk device, in order.
    pub(crate) async fn finish(
        &self,
        frozen: Option<FrozenUpper>,
        fork_ids: &[String],
    ) -> Result<Vec<Option<String>>, VmmError> {
        for (_, copies) in &self.copies {
            if let [first, rest @ ..] = copies.as_slice() {
                for copy in rest {
                    let (first, copy) = (first.clone(), copy.clone());
                    blocking("copying a data disk", move || {
                        crate::storage::files::copy_sparse(&first, &copy)
                    })
                    .await?;
                }
            }
        }
        match (self.oci, frozen) {
            (Some(oci), Some(frozen)) => {
                let devices = mount_fork_disks(&oci.overlaybd, &frozen, fork_ids).await;
                // The forks that got a disk have their own link to the layer.
                oci.overlaybd.release(frozen).await;
                Ok(devices?.into_iter().map(Some).collect())
            }
            _ => Ok(vec![None; fork_ids.len()]),
        }
    }
}

/// Seal the frozen layer and mount a disk for each fork on it, one at a
/// time (finding a new TCMU device is serial anyway). None is left mounted
/// if one fails.
pub(crate) async fn mount_fork_disks(
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

/// Give a fork, `config`, a cloud-init seed image of its own at `seed_path`
/// if its source has one: the same content, pointed to by its drive. Returns
/// its path.
pub(crate) async fn give_fork_its_seed(
    config: &mut ProtoVmConfig,
    seed_path: &Path,
) -> Result<Option<String>, VmmError> {
    let Some(ci) = cloud_init::seed_config(config).cloned() else {
        return Ok(None);
    };
    cloud_init::write_seed_image(seed_path, &ci).await?;
    let seed = seed_path.display().to_string();
    for drive in config
        .disks
        .iter_mut()
        .filter(|d| d.id == cloud_init::SEED_DRIVE)
    {
        drive.path = Some(seed.clone());
    }
    Ok(Some(seed))
}

/// Tell a fork's guest, behind `vsock_path`, that it was forked, and its NICs'
/// addresses if it has NICs (`readdress`, a [`readdress_request`]).
pub(crate) async fn tell_fork(vsock_path: &Path, readdress: Option<&[u8]>) -> Result<(), VmmError> {
    send_fork_notice(vsock_path).await?;
    if let Some(readdress) = readdress {
        readdress_guest(vsock_path, readdress).await?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::node::{CloudInitConfig, DiskConfig, DiskCopy, NetConfig, VsockConfig};

    const SEED: &str = cloud_init::SEED_DRIVE;

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
            disks: vec![],
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

    /// A node's OverlayBD manager, for a VM it maps an image for.
    fn overlaybd(dir: &tempfile::TempDir) -> Option<Arc<OverlayBdManager>> {
        Some(Arc::new(OverlayBdManager::new(
            "unused-convertor",
            dir.path().join("overlaybd"),
        )))
    }

    #[tokio::test]
    async fn a_forks_data_disks_are_copies_and_read_only_ones_are_shared() {
        let dir = tempfile::TempDir::new().unwrap();
        let oci = overlaybd(&dir);
        let source = ProtoVmConfig {
            disks: vec![
                disk("rootfs"),
                disk("data"),
                DiskConfig {
                    readonly: Some(true),
                    ..disk("iso")
                },
            ],
            ..with_guest_agent()
        };
        let fork = |vm_id: &str, copies: &[&str]| Fork {
            vm_id: vm_id.into(),
            disks: copies
                .iter()
                .map(|id| DiskCopy {
                    drive_id: id.to_string(),
                    path: format!("/pool/{vm_id}-{id}"),
                })
                .collect(),
            ..Default::default()
        };
        let forks = [fork("fork-1", &["data"]), fork("fork-2", &["data"])];

        let data = data_disks(&source, &forks).unwrap();
        let copied: Vec<_> = data
            .iter()
            .map(|disk| (disk.drive_id.as_str(), disk.path.as_path()))
            .collect();
        assert_eq!(copied, [("data", Path::new("/dev/data"))]);
        // What is left is the OCI image's disk.
        let found = source_disk("source", &source, &data, oci.clone())
            .unwrap()
            .unwrap();
        assert_eq!(found.drive_id, "rootfs");
        let config = fork_config(&source, &forks[0]).unwrap();
        let paths: Vec<_> = config.disks.iter().map(|d| d.path.as_deref()).collect();
        assert_eq!(
            paths,
            [
                Some("/dev/rootfs"),
                Some("/pool/fork-1-data"),
                Some("/dev/iso")
            ]
        );

        // A writable disk that isn't copied, siblings that copy different
        // disks, and a copy of a disk the source doesn't have.
        assert!(matches!(
            source_disk("source", &source, &[], oci),
            Err(VmmError::InvalidConfig(_))
        ));
        assert!(matches!(
            data_disks(&source, &[fork("a", &["data"]), fork("b", &[])]),
            Err(VmmError::InvalidConfig(_))
        ));
        assert!(matches!(
            fork_config(&source, &fork("c", &["none"])),
            Err(VmmError::InvalidConfig(_))
        ));
    }

    #[tokio::test]
    async fn a_fork_gets_a_cloud_init_seed_of_its_own() {
        let dir = tempfile::TempDir::new().unwrap();
        let ci = CloudInitConfig {
            user_data: "#cloud-config\n".into(),
            meta_data: "instance-id: source\n".into(),
            ..Default::default()
        };
        let source_seed = dir.path().join("source-cidata.img");
        let fork_seed = dir.path().join("fork-cidata.img");
        let mut config = ProtoVmConfig {
            vm_id: "fork".into(),
            cloud_init: Some(ci.clone()),
            disks: vec![DiskConfig {
                id: SEED.into(),
                path: Some(source_seed.display().to_string()),
                readonly: Some(true),
                ..Default::default()
            }],
            ..with_guest_agent()
        };

        let seed = give_fork_its_seed(&mut config, &fork_seed)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(seed, fork_seed.display().to_string());
        assert_eq!(config.disks[0].path.as_deref(), Some(seed.as_str()));
        assert_eq!(
            std::fs::read(&seed).unwrap(),
            crate::cloud_init::build_seed_image(&ci.user_data, &ci.meta_data, None).unwrap()
        );

        // Without cloud-init, nothing.
        let mut without = with_guest_agent();
        assert!(
            give_fork_its_seed(&mut without, &fork_seed)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_fork_gets_its_sources_oci_disk_and_no_other() {
        let dir = tempfile::TempDir::new().unwrap();
        let oci = overlaybd(&dir);
        let with_disks = |disks| ProtoVmConfig {
            disks,
            ..with_guest_agent()
        };

        let none = source_disk("source", &with_disks(vec![]), &[], oci.clone());
        assert!(matches!(none, Ok(None)));

        let found = source_disk(
            "source",
            &with_disks(vec![disk("rootfs")]),
            &[],
            oci.clone(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            (found.drive_id.as_str(), found.device.as_str()),
            ("rootfs", "/dev/rootfs")
        );

        // Its cloud-init seed is each fork's own.
        let with_seed = ProtoVmConfig {
            cloud_init: Some(CloudInitConfig {
                user_data: "#cloud-config\n".into(),
                ..Default::default()
            }),
            ..with_disks(vec![disk("rootfs"), disk(SEED)])
        };
        let found = source_disk("source", &with_seed, &[], oci.clone())
            .unwrap()
            .unwrap();
        assert_eq!(found.drive_id, "rootfs");

        // A disk OverlayBD didn't map for the VM (on a node without
        // OverlayBD, say), or a second disk.
        for (oci, disks) in [
            (None, vec![disk("rootfs")]),
            (oci.clone(), vec![disk("rootfs"), disk("data")]),
            (oci.clone(), vec![disk("rootfs"), disk(SEED)]),
        ] {
            assert!(matches!(
                source_disk("source", &with_disks(disks), &[], oci),
                Err(VmmError::InvalidConfig(_))
            ));
        }
    }
}
