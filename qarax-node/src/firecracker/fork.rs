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
//! Only VMs without disks or NICs fork: a fork would write to its source's
//! disk files, and its NICs would have the source's MAC and IP addresses.

use std::io::Read;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use firecracker_rust_sdk::models::VsockOverride;
use futures::future::join_all;

use super::*;

/// How long a fork's guest agent has to answer the fork notice once it resumes.
const FORK_NOTICE_TIMEOUT: Duration = Duration::from_secs(10);

/// Bytes of entropy each fork gets.
const FORK_ENTROPY_BYTES: usize = 64;

/// Refuse a VM a fork would share more than memory with.
fn check_forkable(config: &ProtoVmConfig) -> Result<(), VmmError> {
    if !config.networks.is_empty() {
        return Err(VmmError::InvalidConfig(
            "can't fork a VM with network interfaces: its forks would have their MAC and IP \
             addresses"
                .into(),
        ));
    }
    if !config.disks.is_empty() {
        return Err(VmmError::InvalidConfig(
            "can't fork a VM with disks: its forks would write to the same disk files".into(),
        ));
    }
    if config.vsock.is_none() {
        return Err(VmmError::InvalidConfig(
            "can't fork a VM without the guest agent, which reseeds each fork's random generator"
                .into(),
        ));
    }
    Ok(())
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

impl FirecrackerManager {
    pub(super) async fn fork(&self, source_id: &str, fork_ids: &[String]) -> Result<(), VmmError> {
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

        let snapshot_dir = self
            .runtime_dir
            .join(format!("{source_id}.fork-{}", uuid::Uuid::new_v4()));
        let forked = self
            .fork_from_snapshot(source_id, &source_config, fork_ids, &snapshot_dir)
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
        info!("FC: VM {} forked into {} VMs", source_id, fork_ids.len());
        Ok(())
    }

    /// Snapshot `source_id` into `dir` and start every fork from it. Either
    /// all of them start, or none is left running.
    async fn fork_from_snapshot(
        &self,
        source_id: &str,
        source_config: &ProtoVmConfig,
        fork_ids: &[String],
        dir: &Path,
    ) -> Result<Vec<(String, FcVmInstance)>, VmmError> {
        self.snapshot_for_fork(source_id, dir).await?;

        let started = join_all(
            fork_ids
                .iter()
                .map(|fork_id| self.start_fork(fork_id, source_config, dir)),
        )
        .await;
        let mut forks = Vec::with_capacity(started.len());
        let mut first_error = None;
        for (fork_id, result) in fork_ids.iter().zip(started) {
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
                self.discard_fork(&fork_id, instance.process).await;
            }
            return Err(e);
        }
        Ok(forks)
    }

    /// Pause `source_id` if it runs, write a full snapshot of it to `dir`,
    /// and resume it.
    async fn snapshot_for_fork(&self, source_id: &str, dir: &Path) -> Result<(), VmmError> {
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
        let snapshot = micro_vm
            .create_snapshot(&SnapshotCreateParams::new(
                dir.join("mem.snap").display().to_string(),
                dir.join("vm.snap").display().to_string(),
            ))
            .await
            .map_err(sdk_err);
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

    /// Start one fork from the snapshot in `dir` and give its guest the fork
    /// notice.
    async fn start_fork(
        &self,
        fork_id: &str,
        source_config: &ProtoVmConfig,
        dir: &Path,
    ) -> Result<FcVmInstance, VmmError> {
        let mut config = source_config.clone();
        config.vm_id = fork_id.to_string();
        let vsock_path = self.vsock_socket_path(fork_id);
        if let Some(vsock) = config.vsock.as_mut() {
            vsock.socket = Some(vsock_path.display().to_string());
        }

        let process = self.spawn_process(fork_id).await?;
        let socket_path = self.socket_path(fork_id);
        let started = async {
            let machine = Machine::attach(&socket_path).await.map_err(sdk_err)?;
            let micro_vm = machine
                .load_and_resume(&SnapshotLoadParams {
                    snapshot_path: dir.join("vm.snap").display().to_string(),
                    mem_backend: Some(Box::new(MemoryBackend::new(
                        BackendType::File,
                        dir.join("mem.snap").display().to_string(),
                    ))),
                    vsock_override: Some(Box::new(VsockOverride::new(
                        vsock_path.display().to_string(),
                    ))),
                    resume_vm: Some(true),
                    ..Default::default()
                })
                .await
                .map_err(sdk_err)?;
            Self::send_fork_notice(&vsock_path).await?;
            Ok(micro_vm)
        }
        .await;

        match started {
            Ok(micro_vm) => Ok(FcVmInstance {
                proto_config: config,
                process: Some(process),
                socket_path,
                tap_devices: vec![],
                phase: FcPhase::Live {
                    micro_vm,
                    state: LiveState::Running,
                },
            }),
            Err(e) => {
                self.discard_fork(fork_id, Some(process)).await;
                Err(e)
            }
        }
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
            match Self::guest_agent_request(vsock_path, &notice, ATTEMPT_TIMEOUT).await {
                Ok(response) => {
                    let ack: GuestAgentAck = serde_json::from_slice(&response).map_err(|e| {
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

    /// Stop a fork that won't be kept and remove its sockets. Its log stays,
    /// as a deleted VM's does.
    async fn discard_fork(&self, fork_id: &str, process: Option<Child>) {
        if let Some(mut process) = process
            && let Err(e) = process.kill().await
        {
            warn!("FC: Failed to kill fork {}: {}", fork_id, e);
        }
        for path in [self.socket_path(fork_id), self.vsock_socket_path(fork_id)] {
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
    use crate::rpc::node::{DiskConfig, NetConfig, VsockConfig};

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
    fn a_vm_with_a_nic_a_disk_or_no_guest_agent_does_not_fork() {
        let refused = [
            ProtoVmConfig {
                networks: vec![NetConfig::default()],
                ..with_guest_agent()
            },
            ProtoVmConfig {
                disks: vec![DiskConfig::default()],
                ..with_guest_agent()
            },
            ProtoVmConfig {
                vsock: None,
                ..with_guest_agent()
            },
        ];
        for config in refused {
            assert!(matches!(
                check_forkable(&config),
                Err(VmmError::InvalidConfig(_))
            ));
        }
    }
}
