//! Forking a Cloud Hypervisor VM ([`crate::vmm::fork`] has what doesn't
//! depend on the hypervisor).
//!
//! The source is paused, snapshotted and resumed. Each fork is a new Cloud
//! Hypervisor process that restores the snapshot copy-on-write
//! (`memory_restore_mode=copyonwrite`): its memory maps the snapshot's memory
//! file privately, so the forks share the source's pages until they write
//! them, and the file has to stay as it is for as long as the fork runs. Each
//! fork restores from a directory of its own, `<fork>.fork`, which holds hard
//! links to the snapshot's memory and device state, and the snapshot's config
//! rewritten for the fork: its own vsock socket, TAPs and disk files. The
//! directory goes with the fork.
//!
//! Cloud Hypervisor restores a VM paused: the forks are resumed in a second
//! step, so that their firewall is in place before they run.

use futures::future::join_all;

use super::*;
use crate::overlaybd::FrozenUpper;
use crate::rpc::node::Fork;
use crate::vmm::VmmError;
use crate::vmm::fork::{
    ForkDisks, SourceDisk, check_forkable, data_disks, fork_config, give_fork_its_seed,
    readdress_request, source_disk, tell_fork,
};

/// The snapshot config a fork restores from: its source's, `snapshot`, on the
/// fork's own vsock socket, TAPs (with their host-side addresses) and disk
/// files, from `fork`, its config. Its NICs' MACs and kernel command line are
/// the fork's too, for when its guest boots again; until then the restored
/// devices keep what the guest had. A serial port or console on a PTY gets a
/// PTY of its own, and one on a file or socket the fork's, the source's ID in
/// its name replaced by the fork's.
fn fork_snapshot_config(
    mut snapshot: serde_json::Value,
    source_id: &str,
    fork: &ProtoVmConfig,
    vsock_socket: &Path,
) -> Result<serde_json::Value, VmmError> {
    let invalid = |what: String| VmmError::InvalidConfig(format!("fork {}: {what}", fork.vm_id));
    for disk in snapshot["disks"].as_array_mut().into_iter().flatten() {
        let id = disk["id"].as_str().unwrap_or_default().to_string();
        let path = fork
            .disks
            .iter()
            .find(|drive| drive.id == id)
            .and_then(|drive| drive.path.clone())
            .ok_or_else(|| invalid(format!("no file for disk {id:?}")))?;
        disk["path"] = path.into();
    }
    for net in snapshot["net"].as_array_mut().into_iter().flatten() {
        let id = net["id"].as_str().unwrap_or_default().to_string();
        let nic = fork
            .networks
            .iter()
            .find(|nic| nic.id == id)
            .ok_or_else(|| invalid(format!("no NIC {id:?}")))?;
        let entry = net
            .as_object_mut()
            .ok_or_else(|| invalid(format!("NIC {id:?}")))?;
        for (key, value) in [
            ("tap", &nic.tap),
            ("mac", &nic.mac),
            ("ip", &nic.ip),
            ("mask", &nic.mask),
            ("host_mac", &nic.host_mac),
        ] {
            match value {
                Some(value) => entry.insert(key.into(), value.clone().into()),
                None => entry.remove(key),
            };
        }
    }
    if let Some(cmdline) = fork
        .payload
        .as_ref()
        .and_then(|payload| payload.cmdline.as_ref())
        && snapshot["payload"].is_object()
    {
        snapshot["payload"]["cmdline"] = cmdline.clone().into();
    }
    if let Some(vsock) = snapshot.get_mut("vsock").filter(|vsock| !vsock.is_null()) {
        vsock["socket"] = vsock_socket.display().to_string().into();
    }
    for device in ["serial", "console"] {
        let entry = &mut snapshot[device];
        match entry["mode"].as_str() {
            // A PTY of its own: Cloud Hypervisor records the one it opened.
            Some("Pty") => {
                if let Some(entry) = entry.as_object_mut() {
                    entry.remove("file");
                }
            }
            Some("File" | "Socket") => {
                for key in ["file", "socket"] {
                    if let Some(path) = entry[key].as_str() {
                        if !path.contains(source_id) {
                            return Err(invalid(format!(
                                "its {device} writes to {path}, which isn't named for its source"
                            )));
                        }
                        entry[key] = path.replace(source_id, &fork.vm_id).into();
                    }
                }
            }
            _ => {}
        }
    }
    Ok(snapshot)
}

impl VmManager {
    /// The directory a fork restored from, which it needs for as long as it
    /// runs: its memory maps the snapshot's memory file there.
    pub(super) fn fork_dir(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{vm_id}.fork"))
    }

    pub(super) async fn fork(&self, source_id: &str, forks: &[Fork]) -> Result<(), VmmError> {
        let fork_ids: Vec<String> = forks.iter().map(|fork| fork.vm_id.clone()).collect();
        info!("Forking VM {} into {:?}", source_id, fork_ids);
        if fork_ids.is_empty() {
            return Err(VmmError::InvalidConfig("no VMs to fork into".into()));
        }
        let adding = self
            .vms
            .reserve(fork_ids.clone())
            .map_err(VmmError::VmAlreadyExists)?;
        let (source_config, oci_mapped) = {
            let source = self.lock_vm(source_id).await?;
            (
                source.config(),
                source.storage_backend_kind == Some(StoragePoolKind::Overlaybd),
            )
        };
        check_forkable(&source_config)?;
        let configs = forks
            .iter()
            .map(|fork| fork_config(&source_config, fork))
            .collect::<Result<Vec<_>, _>>()?;
        let data = data_disks(&source_config, forks)?;
        let oci = self.overlaybd_manager.clone().filter(|_| oci_mapped);
        let disk = source_disk(source_id, &source_config, &data, oci)?;
        let disks = ForkDisks::new(disk.as_ref(), &data, &configs);

        let snapshot_dir = self
            .runtime_dir
            .join(format!("{source_id}.fork-{}", Uuid::new_v4()));
        let forked = async {
            disks.write_back(source_id).await?;
            let frozen = self
                .snapshot_for_fork(source_id, &disks, &snapshot_dir)
                .await?;
            let devices = disks.finish(frozen, &fork_ids).await?;
            self.restore_forks(source_id, configs, disk.as_ref(), &devices, &snapshot_dir)
                .await
        }
        .await;
        // Each fork has links to the snapshot's files in a directory of its own.
        if let Err(e) = tokio::fs::remove_dir_all(&snapshot_dir).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            warn!(
                "Failed to remove fork snapshot {}: {}",
                snapshot_dir.display(),
                e
            );
        }
        let forks = forked?;

        for (fork_id, instance) in &forks {
            self.persist_config(fork_id, &instance.config()).await;
        }
        adding.finish(
            forks
                .into_iter()
                .map(|(fork_id, instance)| (fork_id, ManagedVm::new(instance))),
        );
        info!(
            "VM {} forked into {} VMs, paused",
            source_id,
            fork_ids.len()
        );
        Ok(())
    }

    /// Resume forks [`Self::fork`] restored, and tell each one's guest that it
    /// is a fork and, if it has NICs, their addresses.
    pub(super) async fn resume_forks(
        &self,
        source_id: &str,
        fork_ids: &[String],
    ) -> Result<(), VmmError> {
        let config = |vm_id: &str| {
            self.vms
                .get(vm_id, |vm| vm.summary.borrow().config.clone())
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
                    VmManager::resume_vm(self, fork_id).await?;
                    tell_fork(&vsock_path, readdress.as_deref()).await
                }),
        )
        .await
        .into_iter()
        .collect()
    }

    /// Pause `source_id` if it runs, snapshot it into `dir`, freeze its
    /// `disks`, and resume it.
    async fn snapshot_for_fork(
        &self,
        source_id: &str,
        disks: &ForkDisks<'_>,
        dir: &Path,
    ) -> Result<Option<FrozenUpper>, VmmError> {
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            VmmError::ProcessError(format!(
                "Failed to create snapshot directory {}: {}",
                dir.display(),
                e
            ))
        })?;

        let source = self.lock_vm(source_id).await?;
        let was_running = match source.status() {
            VmStatus::Running => true,
            VmStatus::Paused => false,
            _ => return Err(VmmError::ProcessError("VM is not running".into())),
        };
        let socket = source.socket_path.clone();
        if was_running {
            Self::send_api_request(&socket, "PUT", "/api/v1/vm.pause", None).await?;
            source.set_status(VmStatus::Paused);
        }
        let snapshot = async {
            let body =
                serde_json::json!({ "destination_url": format!("file://{}", dir.display()) })
                    .to_string();
            Self::send_api_request(&socket, "PUT", "/api/v1/vm.snapshot", Some(&body)).await?;
            disks.freeze(source_id).await
        }
        .await;
        if was_running {
            // Even when the snapshot failed: a fork mustn't leave its source paused.
            Self::send_api_request(&socket, "PUT", "/api/v1/vm.resume", None).await?;
            source.set_status(VmStatus::Running);
        }
        snapshot
    }

    /// Restore every fork, `configs`, from the snapshot in `dir`, paused, each
    /// with its OCI disk on its own `devices` entry. Either all of them are
    /// restored, or none is left.
    async fn restore_forks(
        &self,
        source_id: &str,
        configs: Vec<ProtoVmConfig>,
        disk: Option<&SourceDisk>,
        devices: &[Option<String>],
        dir: &Path,
    ) -> Result<Vec<(String, VmInstance)>, VmmError> {
        let fork_ids: Vec<String> = configs.iter().map(|config| config.vm_id.clone()).collect();
        let restored = join_all(configs.into_iter().zip(devices).map(|(config, device)| {
            let disk = disk.zip(device.as_deref());
            self.restore_fork(source_id, config, disk, dir)
        }))
        .await;
        let mut forks = Vec::with_capacity(restored.len());
        let mut first_error = None;
        for (fork_id, result) in fork_ids.iter().zip(restored) {
            match result {
                Ok(instance) => forks.push((fork_id.clone(), instance)),
                Err(e) => {
                    warn!("Fork {} of VM {} failed: {}", fork_id, source_id, e);
                    first_error.get_or_insert(e);
                }
            }
        }
        if let Some(e) = first_error {
            for (fork_id, mut instance) in forks {
                self.discard_fork(&fork_id, instance.process.take(), &instance.tap_devices)
                    .await;
            }
            return Err(e);
        }
        Ok(forks)
    }

    /// Restore one fork, `config`, paused, from the snapshot in `dir`: onto
    /// TAPs of its own, with `disk`'s drive on the fork's own device.
    async fn restore_fork(
        &self,
        source_id: &str,
        mut config: ProtoVmConfig,
        disk: Option<(&SourceDisk, &str)>,
        dir: &Path,
    ) -> Result<VmInstance, VmmError> {
        let fork_id = config.vm_id.clone();
        if let Some((disk, device)) = disk {
            for drive in config.disks.iter_mut().filter(|d| d.id == disk.drive_id) {
                drive.path = Some(device.to_string());
            }
        }
        let vsock_path = self.vsock_socket_path(&fork_id);
        if let Some(vsock) = config.vsock.as_mut() {
            vsock.socket = Some(vsock_path.display().to_string());
        }
        let tap_devices = self
            .create_fork_taps(&fork_id, &mut config.networks)
            .await?;

        let mut process = None;
        let restored = async {
            give_fork_its_seed(&mut config, &self.cloud_init_seed_path(&fork_id)).await?;
            let fork_dir = self.fork_dir(&fork_id);
            self.link_snapshot(dir, &fork_dir, source_id, &config, &vsock_path)
                .await?;

            let socket_path = self.socket_path(&fork_id);
            let log_file = tokio::fs::File::create(self.log_path(&fork_id))
                .await
                .map_err(VmmError::SpawnError)?
                .into_std()
                .await;
            process = Some(
                self.spawn_cloud_hypervisor(&fork_id, log_file)
                    .map_err(VmmError::SpawnError)?,
            );
            Self::wait_for_api_socket(&socket_path).await?;
            // Copy-on-write: the forks share the snapshot's memory pages.
            let body = serde_json::json!({
                "source_url": format!("file://{}", fork_dir.display()),
                "memory_restore_mode": "CopyOnWrite",
                "resume": false,
            })
            .to_string();
            Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.restore", Some(&body)).await?;

            let vm = Machine::connect(MachineConfig {
                vm_id: Uuid::parse_str(&fork_id)
                    .map_err(|e| VmmError::InvalidConfig(format!("Invalid VM ID: {e}")))?,
                socket_path: Cow::Owned(socket_path.clone()),
                exec_path: Cow::Owned(self.ch_binary.clone()),
            })
            .await
            .map_err(VmManagerError::SdkError)?;
            let (serial_pty_path, console_pty_path) =
                self.query_pty_paths(&socket_path, &config).await;
            Ok::<_, VmmError>((vm, socket_path, serial_pty_path, console_pty_path))
        }
        .await;

        match restored {
            Ok((vm, socket_path, serial_pty_path, console_pty_path)) => Ok(VmInstance {
                summary: watch::Sender::new(VmSummary {
                    config,
                    status: VmStatus::Paused,
                }),
                process,
                vm,
                socket_path,
                tap_devices,
                passt_processes: vec![],
                serial_pty_path,
                console_pty_path,
                vsock_socket_path: Some(vsock_path),
                storage_backend_kind: disk.map(|_| StoragePoolKind::Overlaybd),
            }),
            Err(e) => {
                self.discard_fork(&fork_id, process, &tap_devices).await;
                Err(e)
            }
        }
    }

    /// Create a TAP for each of a fork's `networks`, named for `fork_id`, and
    /// attach the TAPs to their bridges. Returns the TAPs, of which none is
    /// left if it fails.
    async fn create_fork_taps(
        &self,
        fork_id: &str,
        networks: &mut [ProtoNetConfig],
    ) -> Result<Vec<String>, VmmError> {
        let mut tap_devices = Vec::new();
        let created = async {
            for (i, net) in networks.iter_mut().enumerate() {
                let tap_name = Self::tap_name_for_net(fork_id, i);
                Self::create_tap_device(&tap_name).await?;
                net.tap = Some(tap_name.clone());
                tap_devices.push(tap_name);
            }
            for net in networks.iter() {
                if let (Some(tap_name), Some(bridge_name)) = (&net.tap, &net.bridge) {
                    crate::networking::bridge::attach_to_bridge(tap_name, bridge_name)
                        .await
                        .map_err(|e| {
                            VmmError::TapError(format!(
                                "Failed to attach TAP {tap_name} to bridge {bridge_name}: {e}"
                            ))
                        })?;
                }
            }
            Ok::<_, VmmError>(())
        }
        .await;
        if let Err(e) = created {
            for tap in &tap_devices {
                Self::delete_tap_device(tap).await;
            }
            return Err(e);
        }
        Ok(tap_devices)
    }

    /// Make `fork_dir`, which a fork restores from: hard links to the files of
    /// the snapshot in `dir` (its memory, which the fork maps, and its device
    /// state), and the snapshot's config for the fork.
    async fn link_snapshot(
        &self,
        dir: &Path,
        fork_dir: &Path,
        source_id: &str,
        config: &ProtoVmConfig,
        vsock_socket: &Path,
    ) -> Result<(), VmmError> {
        let io = |what: &str, e: std::io::Error| {
            VmmError::ProcessError(format!("Failed to {what} for fork {}: {e}", config.vm_id))
        };
        tokio::fs::create_dir(fork_dir)
            .await
            .map_err(|e| io("create its snapshot directory", e))?;
        let mut entries = tokio::fs::read_dir(dir)
            .await
            .map_err(|e| io("read the snapshot", e))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| io("read the snapshot", e))?
        {
            if entry.file_name() != "config.json" {
                tokio::fs::hard_link(entry.path(), fork_dir.join(entry.file_name()))
                    .await
                    .map_err(|e| io("link the snapshot", e))?;
            }
        }
        let snapshot: serde_json::Value = serde_json::from_slice(
            &tokio::fs::read(dir.join("config.json"))
                .await
                .map_err(|e| io("read the snapshot's config", e))?,
        )
        .map_err(|e| VmmError::ProcessError(format!("Invalid snapshot config: {e}")))?;
        let forked = fork_snapshot_config(snapshot, source_id, config, vsock_socket)?;
        tokio::fs::write(
            fork_dir.join("config.json"),
            serde_json::to_vec(&forked)
                .map_err(|e| VmmError::ProcessError(format!("Invalid fork config: {e}")))?,
        )
        .await
        .map_err(|e| io("write its config", e))
    }

    /// Stop a fork that won't be kept, and remove its TAPs, sockets, snapshot
    /// directory, seed and OCI disk. Its log stays, as a deleted VM's does.
    async fn discard_fork(&self, fork_id: &str, process: Option<Child>, taps: &[String]) {
        if let Some(mut process) = process
            && let Err(e) = process.kill().await
        {
            warn!("Failed to kill fork {}: {}", fork_id, e);
        }
        for tap in taps {
            Self::delete_tap_device(tap).await;
        }
        if let Some(backend) = self.storage_backends.get(StoragePoolKind::Overlaybd)
            && let Err(e) = backend.unmap(fork_id).await
        {
            warn!("Failed to unmap the OCI disk of fork {}: {}", fork_id, e);
        }
        for path in [
            self.socket_path(fork_id),
            self.vsock_socket_path(fork_id),
            self.cloud_init_seed_path(fork_id),
        ] {
            if let Err(e) = tokio::fs::remove_file(&path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                warn!("Failed to remove {}: {}", path.display(), e);
            }
        }
        if let Err(e) = tokio::fs::remove_dir_all(self.fork_dir(fork_id)).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            warn!("Failed to remove the snapshot of fork {}: {}", fork_id, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::node::DiskConfig;

    #[test]
    fn a_fork_restores_on_its_own_socket_taps_and_disks() {
        let snapshot = serde_json::json!({
            "cpus": {"boot_vcpus": 1},
            "payload": {"kernel": "/vmlinux", "cmdline": "ip=10.0.0.2::10.0.0.1:255.255.255.0::eth0:off:"},
            "disks": [{"id": "rootfs", "path": "/dev/sda"}, {"id": "data", "path": "/pool/a"}],
            "net": [{"id": "net0", "tap": "qtsource0", "ip": "10.0.0.2", "mask": "255.255.255.0",
                     "mac": "02:00:00:00:00:01", "host_mac": "02:00:00:00:01:01"}],
            "vsock": {"cid": 3, "socket": "/run/source.vsock"},
            "serial": {"mode": "Pty", "file": "/dev/pts/0"},
            "console": {"mode": "File", "file": "/run/source.console.log"},
        });
        let disk = |id: &str, path: &str| DiskConfig {
            id: id.into(),
            path: Some(path.into()),
            ..Default::default()
        };
        let fork = ProtoVmConfig {
            vm_id: "fork".into(),
            disks: vec![disk("rootfs", "/dev/sdb"), disk("data", "/pool/b")],
            networks: vec![ProtoNetConfig {
                id: "net0".into(),
                tap: Some("qtfork0".into()),
                ip: Some("10.0.0.3".into()),
                mask: Some("255.255.255.0".into()),
                mac: Some("02:00:00:00:00:02".into()),
                ..Default::default()
            }],
            payload: Some(crate::rpc::node::PayloadConfig {
                cmdline: Some("ip=10.0.0.3::10.0.0.1:255.255.255.0::eth0:off:".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let config = fork_snapshot_config(
            snapshot.clone(),
            "source",
            &fork,
            Path::new("/run/fork.vsock"),
        )
        .unwrap();
        assert_eq!(config["disks"][0]["path"], "/dev/sdb");
        assert_eq!(config["disks"][1]["path"], "/pool/b");
        assert_eq!(config["net"][0]["tap"], "qtfork0");
        assert_eq!(config["net"][0]["ip"], "10.0.0.3");
        // For when it boots again; the TAP has a MAC of its own.
        assert_eq!(config["net"][0]["mac"], "02:00:00:00:00:02");
        assert!(config["net"][0].get("host_mac").is_none());
        assert_eq!(
            config["payload"]["cmdline"],
            "ip=10.0.0.3::10.0.0.1:255.255.255.0::eth0:off:"
        );
        assert_eq!(config["payload"]["kernel"], "/vmlinux");
        assert_eq!(config["vsock"]["socket"], "/run/fork.vsock");
        assert!(config["serial"].get("file").is_none());
        assert_eq!(config["console"]["file"], "/run/fork.console.log");
        assert_eq!(config["cpus"], snapshot["cpus"]);

        // A disk the fork has no file for, or a log not named for the source.
        let missing = ProtoVmConfig {
            disks: vec![disk("rootfs", "/dev/sdb")],
            ..fork.clone()
        };
        let mut shared_log = snapshot.clone();
        shared_log["console"]["file"] = "/run/console.log".into();
        for (snapshot, fork) in [(snapshot, &missing), (shared_log, &fork)] {
            assert!(matches!(
                fork_snapshot_config(snapshot, "source", fork, Path::new("/run/fork.vsock")),
                Err(VmmError::InvalidConfig(_))
            ));
        }
    }
}
