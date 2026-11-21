//! Firecracker VMM backend.
//!
//! Implements `VmmManager` for Firecracker microVMs using the
//! `firecracker-rust-sdk` crate, which provides typed models and an HTTP/1.1
//! client over the Firecracker Unix API socket.
//!
//! Lifecycle mapping:
//!   create_vm   — map an OCI image disk through OverlayBD, then spawn the
//!                 process + configure via SDK (Machine)
//!   start_vm    — Machine::start_instance() → MicroVm; a stopped VM gets
//!                 a fresh process first, since a microVM can't boot twice
//!   stop_vm     — shut down through the guest agent, else kill the process
//!   force_stop  — kill the process
//!   pause_vm    — MicroVm::patch_vm(Paused)
//!   resume_vm   — MicroVm::patch_vm(Resumed)
//!   delete_vm   — kill + cleanup
//!   snapshot_vm — MicroVm::create_snapshot()
//!   restore_vm  — spawn + Machine::load_and_resume()
//!   fork_vm     — snapshot, then load it into new processes (see `fork`)

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use firecracker_rust_sdk::machine::{Machine, MicroVm};
use firecracker_rust_sdk::models::{
    BootSource, Drive, MachineConfiguration, MemoryBackend, NetworkInterface, RateLimiter,
    SnapshotCreateParams, SnapshotLoadParams, TokenBucket, Vm, Vsock,
    instance_info::State as FcInstanceState, memory_backend::BackendType, vm::State as FcVmState,
};
use prost::Message;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, OwnedMappedMutexGuard, OwnedMutexGuard, watch};
use tracing::{debug, info, warn};

use crate::rpc::node::ExecVmResponse;
use crate::rpc::node::{
    Fork, NetConfig as ProtoNetConfig, RateLimiterConfig as ProtoRateLimiterConfig,
    TokenBucket as ProtoTokenBucket, VmConfig as ProtoVmConfig, VmState, VmStatus,
};
use crate::storage::StorageBackend;
use crate::storage::overlaybd::{OverlayBdBackend, map_oci_disks};
use crate::vmm::{VmmError, VmmManager, guest_agent};

mod exec;
mod fork;
use fork::{ForkMemory, ForkMemoryLock, tracks_dirty_pages};
mod helpers;
#[cfg(test)]
mod tests;

/// Where a Firecracker VM is in its lifecycle, with the SDK handle that
/// step needs. The status reported for the VM is derived from it.
enum FcPhase {
    /// The FC process is configured but `InstanceStart` has not been sent.
    PreBoot(Machine<'static>),
    /// Started (or recovered) and attached; `state` is what FC last reported,
    /// `forking` what its next fork snapshots of its memory.
    Live {
        micro_vm: MicroVm<'static>,
        state: LiveState,
        forking: ForkMemoryLock,
    },
    /// Stopped through qarax. Its FC process is gone or on its way out, and
    /// a start launches a new one.
    Stopped,
    /// Recovered at startup, but its API socket could not be attached.
    Detached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveState {
    Running,
    Paused,
    /// Recovered before `InstanceStart` was sent.
    NotStarted,
    /// FC did not answer the last time it was asked.
    Unknown,
}

impl From<FcInstanceState> for LiveState {
    fn from(state: FcInstanceState) -> Self {
        match state {
            FcInstanceState::Running => LiveState::Running,
            FcInstanceState::Paused => LiveState::Paused,
            FcInstanceState::NotStarted => LiveState::NotStarted,
        }
    }
}

impl FcPhase {
    fn status(&self) -> VmStatus {
        match self {
            FcPhase::PreBoot(_) => VmStatus::Created,
            FcPhase::Live { state, .. } => match state {
                LiveState::Running => VmStatus::Running,
                LiveState::Paused => VmStatus::Paused,
                LiveState::NotStarted => VmStatus::Created,
                LiveState::Unknown => VmStatus::Unknown,
            },
            FcPhase::Stopped => VmStatus::Shutdown,
            FcPhase::Detached => VmStatus::Unknown,
        }
    }
}

struct FcVmInstance {
    proto_config: ProtoVmConfig,
    /// The FC process qarax spawned. `None` for a VM recovered at startup.
    process: Option<Child>,
    socket_path: PathBuf,
    tap_devices: Vec<String>,
    phase: FcPhase,
    /// `phase`'s status as the VM's lock was last released (see [`VmGuard`]).
    published: watch::Sender<VmStatus>,
}

impl FcVmInstance {
    fn new(
        proto_config: ProtoVmConfig,
        process: Option<Child>,
        socket_path: PathBuf,
        tap_devices: Vec<String>,
        phase: FcPhase,
    ) -> Self {
        Self {
            published: watch::Sender::new(phase.status()),
            proto_config,
            process,
            socket_path,
            tap_devices,
            phase,
        }
    }

    fn status(&self) -> VmStatus {
        self.phase.status()
    }

    fn to_vm_state(&self) -> VmState {
        VmState {
            config: Some(self.proto_config.clone()),
            status: self.status().into(),
            memory_actual_size: None,
        }
    }

    /// The attached client of a started VM, or the error for one that isn't.
    fn live(
        &mut self,
        not_live: &str,
    ) -> Result<(&mut MicroVm<'static>, &mut LiveState), VmmError> {
        match &mut self.phase {
            FcPhase::Live {
                micro_vm, state, ..
            } => Ok((micro_vm, state)),
            FcPhase::PreBoot(_) | FcPhase::Stopped | FcPhase::Detached => {
                Err(VmmError::ProcessError(not_live.to_string()))
            }
        }
    }
}

/// A VM the manager knows. Operations on it hold its lock to use or change its
/// instance, some of them across requests to its Firecracker (starting,
/// pausing, resuming or snapshotting it, `get_vm_info`, a fork's snapshot of
/// its source), so a Firecracker that is slow or hung holds up its own VM's
/// operations only. Reads of all the node's VMs take each one's config and
/// last published status from here, without the lock.
struct ManagedVm {
    /// A copy of the instance's: nothing changes a Firecracker VM's config.
    config: ProtoVmConfig,
    status: watch::Receiver<VmStatus>,
    /// `None` once the VM is deleted, for the operations that were waiting.
    instance: Arc<Mutex<Option<FcVmInstance>>>,
}

impl ManagedVm {
    fn new(instance: FcVmInstance) -> Self {
        Self {
            config: instance.proto_config.clone(),
            status: instance.published.subscribe(),
            instance: Arc::new(Mutex::new(Some(instance))),
        }
    }

    fn to_vm_state(&self) -> VmState {
        VmState {
            config: Some(self.config.clone()),
            status: (*self.status.borrow()).into(),
            memory_actual_size: None,
        }
    }
}

/// A VM's instance, while its lock is held. Releasing it publishes the status
/// its phase was left in, so the reads that don't wait for the lock see each
/// VM as the last operation on it left it, as they did when they waited for
/// that operation to end.
struct VmGuard(OwnedMappedMutexGuard<Option<FcVmInstance>, FcVmInstance>);

impl Deref for VmGuard {
    type Target = FcVmInstance;

    fn deref(&self) -> &FcVmInstance {
        &self.0
    }
}

impl DerefMut for VmGuard {
    fn deref_mut(&mut self) -> &mut FcVmInstance {
        &mut self.0
    }
}

impl Drop for VmGuard {
    fn drop(&mut self) {
        self.0.published.send_replace(self.0.phase.status());
    }
}

pub struct FirecrackerManager {
    runtime_dir: PathBuf,
    fc_binary: PathBuf,
    /// The VMs. Locked only to find, add, remove or list them, never across an
    /// await: a std Mutex, which the Send futures here can't hold across one.
    vms: std::sync::Mutex<HashMap<String, ManagedVm>>,
    /// How long `stop_vm` waits for a guest to shut down before killing its
    /// VMM.
    graceful_stop_timeout: Duration,
    /// Presents OCI image disks as block devices; `None` on a node without
    /// OverlayBD.
    overlaybd: Option<Arc<OverlayBdBackend>>,
}

impl FirecrackerManager {
    pub fn new(
        runtime_dir: impl Into<PathBuf>,
        fc_binary: impl Into<PathBuf>,
        graceful_stop_timeout: Duration,
        overlaybd: Option<Arc<OverlayBdBackend>>,
    ) -> Self {
        let runtime_dir = runtime_dir.into();
        let fc_binary = fc_binary.into();
        info!(
            "FirecrackerManager initialized: runtime_dir={}, fc_binary={}",
            runtime_dir.display(),
            fc_binary.display()
        );
        Self {
            runtime_dir,
            fc_binary,
            vms: std::sync::Mutex::new(HashMap::new()),
            graceful_stop_timeout,
            overlaybd,
        }
    }

    /// Release `vm_id`'s OverlayBD device, if it has one.
    async fn unmap_oci_disk(&self, vm_id: &str) {
        if let Some(backend) = &self.overlaybd
            && backend.has_mapping(vm_id).await
            && let Err(e) = backend.unmap(vm_id).await
        {
            warn!(
                "FC: Failed to unmap the OverlayBD disk of VM {}: {}",
                vm_id, e
            );
        }
    }

    pub fn fc_binary(&self) -> &Path {
        &self.fc_binary
    }

    /// Wait for `vm_id`'s lock (see [`ManagedVm`]).
    async fn lock_vm(&self, vm_id: &str) -> Result<VmGuard, VmmError> {
        let not_found = || VmmError::VmNotFound(vm_id.to_string());
        let instance = self
            .vms
            .lock()
            .unwrap()
            .get(vm_id)
            .map(|vm| vm.instance.clone());
        let instance = instance.ok_or_else(not_found)?.lock_owned().await;
        // Deleted while this waited.
        OwnedMutexGuard::try_map(instance, Option::as_mut)
            .map(VmGuard)
            .map_err(|_| not_found())
    }

    /// Take `vm_id` out of the manager once no operation holds its lock; those
    /// waiting for the lock then find the VM gone.
    async fn remove_vm(&self, vm_id: &str) -> Option<FcVmInstance> {
        let instance = self
            .vms
            .lock()
            .unwrap()
            .get(vm_id)
            .map(|vm| vm.instance.clone())?;
        let mut instance = instance.lock().await;
        // Another removal took it while this waited.
        let removed = instance.take()?;
        self.vms.lock().unwrap().remove(vm_id);
        Some(removed)
    }

    /// Launch a Firecracker process for `config` and configure it up to
    /// `InstanceStart`. Dropping the returned process kills it.
    async fn spawn_configured(
        &self,
        vm_id: &str,
        config: &ProtoVmConfig,
    ) -> Result<(Child, Machine<'static>), VmmError> {
        if let Some(path) = Self::vsock_socket_path_from_config(&config.vsock)
            && path.exists()
        {
            let _ = tokio::fs::remove_file(path).await;
        }
        let process = self.spawn_process(vm_id).await?;

        // Machine::attach waits for the socket to be ready.
        let socket_path = self.socket_path(vm_id);
        let mut machine = Machine::attach(&socket_path).await.map_err(sdk_err)?;
        Self::configure_vm(&mut machine, &socket_path, config).await?;
        Ok((process, machine))
    }

    /// Launch a bare Firecracker process on `vm_id`'s API socket, its output
    /// appended to the VM's log. Dropping the returned process kills it.
    async fn spawn_process(&self, vm_id: &str) -> Result<Child, VmmError> {
        let socket_path = self.socket_path(vm_id);
        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }

        // Append, so a restart keeps the log of the boot before it.
        let log_file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path(vm_id))
            .await
            .map_err(VmmError::SpawnError)?
            .into_std()
            .await;
        let stderr_file = log_file.try_clone().map_err(VmmError::SpawnError)?;

        let process = Command::new(&self.fc_binary)
            .arg("--api-sock")
            .arg(&socket_path)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .kill_on_drop(true)
            .spawn()
            .map_err(VmmError::SpawnError)?;
        info!("FC: process started with PID {:?}", process.id());
        Ok(process)
    }

    /// Configure a freshly-attached [`Machine`] from a proto VmConfig using
    /// the SDK's typed API methods.
    async fn configure_vm(
        machine: &mut Machine<'static>,
        api_socket_path: &Path,
        config: &ProtoVmConfig,
    ) -> Result<(), VmmError> {
        let cpus = config.cpus.as_ref().map(|c| c.boot_vcpus).unwrap_or(1);
        let mem_mib = (config
            .memory
            .as_ref()
            .map(|m| m.size)
            .unwrap_or(128 * 1024 * 1024)
            / (1024 * 1024)) as i32;

        let mut machine_config = MachineConfiguration::new(mem_mib, cpus);
        machine_config.track_dirty_pages = Some(tracks_dirty_pages(config));
        machine
            .put_machine_config(&machine_config)
            .await
            .map_err(sdk_err)?;

        if let Some(payload) = &config.payload {
            let kernel = payload.kernel.as_deref().unwrap_or("");
            if !kernel.is_empty() {
                let mut boot = BootSource::new(kernel.to_string());
                if let Some(cmdline) = &payload.cmdline
                    && !cmdline.is_empty()
                {
                    boot.boot_args = Some(cmdline.clone());
                }
                if let Some(initrd) = &payload.initramfs
                    && !initrd.is_empty()
                {
                    boot.initrd_path = Some(initrd.clone());
                }
                machine.put_boot_source(&boot).await.map_err(sdk_err)?;
            }
        }

        let mut has_root = false;
        for disk in &config.disks {
            if let Some(path) = &disk.path {
                let readonly = disk.readonly.unwrap_or(false);
                let is_root = !readonly && !has_root;
                if is_root {
                    has_root = true;
                }
                let mut drive = Drive::new(disk.id.clone(), is_root);
                drive.path_on_host = Some(path.clone());
                drive.is_read_only = Some(readonly);
                drive.rate_limiter = disk.rate_limiter.as_ref().map(rate_limiter_to_sdk);
                machine.put_drive(&disk.id, &drive).await.map_err(sdk_err)?;
                debug!("FC drive {} configured (root={})", disk.id, is_root);
            }
        }

        for net in &config.networks {
            if let Some(tap) = &net.tap {
                let mut iface = NetworkInterface::new(tap.clone(), net.id.clone());
                iface.guest_mac = net.mac.clone();
                // Cloud Hypervisor applies a NIC's limiter to each direction
                // separately; Firecracker takes one per direction.
                iface.rx_rate_limiter = net.rate_limiter.as_ref().map(rate_limiter_to_sdk);
                iface.tx_rate_limiter = iface.rx_rate_limiter.clone();
                machine
                    .put_network_interface(&net.id, &iface)
                    .await
                    .map_err(sdk_err)?;
                debug!("FC network interface {} configured (tap={})", net.id, tap);
            }
        }

        if let Some(vsock) = &config.vsock {
            let guest_cid = vsock.cid.ok_or_else(|| {
                VmmError::InvalidConfig("vsock.cid is required for Firecracker".into())
            })?;
            let guest_cid = i32::try_from(guest_cid).map_err(|_| {
                VmmError::InvalidConfig(format!("vsock.cid {} exceeds i32 range", guest_cid))
            })?;
            let uds_path = vsock.socket.clone().ok_or_else(|| {
                VmmError::InvalidConfig("vsock.socket is required for Firecracker".into())
            })?;

            let mut fc_vsock = Vsock::new(guest_cid, uds_path);
            fc_vsock.vsock_id = vsock.id.clone();
            Self::put_vsock_device(api_socket_path, &fc_vsock).await?;
            debug!("FC vsock configured (cid={guest_cid})");
        }

        Ok(())
    }

    /// Ask the guest agent to shut the guest down and reboot, which ends its
    /// VMM, then wait up to `graceful_stop_timeout` for that. Returns whether
    /// the VMM exited.
    async fn shut_down_guest(&self, vm_id: &str, agent_socket: &Path) -> bool {
        const AGENT_TIMEOUT: Duration = Duration::from_secs(5);
        const POLL_INTERVAL: Duration = Duration::from_millis(250);

        // An older qarax-init answers it as an invalid exec request.
        let accepted = guest_agent::request(
            agent_socket,
            b"{\"shutdown\":{\"reboot\":true}}\n",
            AGENT_TIMEOUT,
        )
        .await
        .and_then(|response| {
            serde_json::from_slice::<guest_agent::Ack>(&response)
                .map_err(|e| VmmError::ExecError(format!("invalid guest-agent response: {e}")))
        });
        match accepted {
            Ok(guest_agent::Ack { exit_code: 0, .. }) => {}
            Ok(guest_agent::Ack { exit_code, .. }) => {
                warn!("FC: VM {vm_id}'s guest agent refused to shut down (exit code {exit_code})");
                return false;
            }
            Err(e) => {
                warn!("FC: VM {vm_id}'s guest agent could not be asked to shut down: {e}");
                return false;
            }
        }

        let deadline = tokio::time::Instant::now() + self.graceful_stop_timeout;
        loop {
            if self.vmm_exited(vm_id).await {
                info!("FC: VM {} guest shut down", vm_id);
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                warn!(
                    "FC: VM {} did not shut down within {}s; killing it",
                    vm_id,
                    self.graceful_stop_timeout.as_secs()
                );
                return false;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Whether the VM's Firecracker process has exited.
    async fn vmm_exited(&self, vm_id: &str) -> bool {
        let Ok(mut instance) = self.lock_vm(vm_id).await else {
            return true;
        };
        match &mut instance.process {
            Some(child) => child.try_wait().ok().flatten().is_some(),
            // Recovered after a node restart: no handle, but nothing answers
            // its API socket once it's gone.
            None => std::os::unix::net::UnixStream::connect(&instance.socket_path).is_err(),
        }
    }
}

fn rate_limiter_to_sdk(limiter: &ProtoRateLimiterConfig) -> Box<RateLimiter> {
    fn bucket(bucket: &ProtoTokenBucket) -> Box<TokenBucket> {
        Box::new(TokenBucket {
            size: bucket.size,
            refill_time: bucket.refill_time,
            one_time_burst: bucket.one_time_burst,
        })
    }
    Box::new(RateLimiter {
        bandwidth: limiter.bandwidth.as_ref().map(bucket),
        ops: limiter.ops.as_ref().map(bucket),
    })
}

fn sdk_err(e: firecracker_rust_sdk::error::Error) -> VmmError {
    VmmError::ProcessError(e.to_string())
}

impl FirecrackerManager {
    /// Create a TAP for each of `networks` that has none, named for `vm_id`,
    /// and attach the TAPs to their bridges. Returns the TAPs it created, of
    /// which none is left if it fails.
    async fn create_taps(
        vm_id: &str,
        networks: &mut [ProtoNetConfig],
    ) -> Result<Vec<String>, VmmError> {
        let mut tap_devices: Vec<String> = Vec::new();
        let created = async {
            for (i, net) in networks.iter_mut().enumerate() {
                if net.tap.is_none() && !net.vhost_user.unwrap_or(false) {
                    let tap_name = Self::tap_name_for_net(vm_id, i);
                    Self::create_tap_device(&tap_name).await?;
                    net.tap = Some(tap_name.clone());
                    tap_devices.push(tap_name);
                }
            }
            for net in networks.iter() {
                if let (Some(tap_name), Some(bridge_name)) = (&net.tap, &net.bridge) {
                    crate::networking::bridge::attach_to_bridge(tap_name, bridge_name)
                        .await
                        .map_err(|e| {
                            VmmError::TapError(format!(
                                "Failed to attach TAP {} to bridge {}: {}",
                                tap_name, bridge_name, e
                            ))
                        })?;
                }
            }
            Ok(())
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
}

#[async_trait::async_trait]
impl VmmManager for FirecrackerManager {
    async fn create_vm(&self, config: ProtoVmConfig) -> Result<VmState, VmmError> {
        let vm_id = config.vm_id.clone();
        info!("FC: Creating VM {}", vm_id);

        if self.vms.lock().unwrap().contains_key(&vm_id) {
            return Err(VmmError::VmAlreadyExists(vm_id));
        }

        tokio::fs::create_dir_all(&self.runtime_dir)
            .await
            .map_err(VmmError::SpawnError)?;

        let mut config = config;

        let tap_devices = Self::create_taps(&vm_id, &mut config.networks).await?;

        if let Some(vsock) = config.vsock.as_mut() {
            self.resolve_vsock_config(&vm_id, vsock);
        }

        if let Some(ci) = crate::cloud_init::seed_config(&config) {
            let seed_path = self.cloud_init_seed_path(&vm_id);
            crate::cloud_init::write_seed_image(&seed_path, ci).await?;
            config.disks.push(crate::rpc::node::DiskConfig {
                id: crate::cloud_init::SEED_DRIVE.to_string(),
                path: Some(seed_path.display().to_string()),
                readonly: Some(true),
                ..Default::default()
            });
            info!("FC: cloud-init seed disk attached for VM {}", vm_id);
        }

        let config_path = self.config_path(&vm_id);

        // Present an OCI image disk as an OverlayBD block device. Last before
        // the spawn, so that only a failed spawn leaves a mapping to undo.
        let oci_mapped = match map_oci_disks(
            self.overlaybd
                .as_deref()
                .map(|backend| backend as &dyn StorageBackend),
            &vm_id,
            &mut config.disks,
        )
        .await
        {
            Ok(mapped) => mapped,
            Err(e) => {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                return Err(VmmError::StorageError(e.to_string()));
            }
        };

        let (process, machine) = match self.spawn_configured(&vm_id, &config).await {
            Ok(spawned) => spawned,
            Err(e) => {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                if oci_mapped {
                    self.unmap_oci_disk(&vm_id).await;
                }
                return Err(e);
            }
        };

        // Persist config for recovery.
        let config_bytes = config.encode_to_vec();
        if let Err(e) = tokio::fs::write(&config_path, config_bytes).await {
            warn!("FC: Failed to persist config for VM {}: {}", vm_id, e);
        }

        let instance = FcVmInstance::new(
            config,
            Some(process),
            self.socket_path(&vm_id),
            tap_devices,
            FcPhase::PreBoot(machine),
        );

        let state = instance.to_vm_state();
        self.vms
            .lock()
            .unwrap()
            .insert(vm_id.clone(), ManagedVm::new(instance));
        info!("FC: VM {} created successfully", vm_id);
        Ok(state)
    }

    async fn start_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Starting VM {}", vm_id);

        let mut instance = self.lock_vm(vm_id).await?;

        // Until the instance starts, a failure leaves the VM stopped, and the
        // next start launches a fresh process.
        let machine = match std::mem::replace(&mut instance.phase, FcPhase::Stopped) {
            FcPhase::PreBoot(machine) => machine,
            FcPhase::Stopped => {
                // A microVM that shut down can't boot again: replace its process.
                if let Some(mut old) = instance.process.take() {
                    let _ = old.kill().await;
                }
                let (process, machine) =
                    self.spawn_configured(vm_id, &instance.proto_config).await?;
                instance.process = Some(process);
                machine
            }
            phase @ FcPhase::Live { .. } => {
                instance.phase = phase;
                return Err(VmmError::ProcessError("VM already started".to_string()));
            }
            FcPhase::Detached => {
                instance.phase = FcPhase::Detached;
                return Err(VmmError::ProcessError(
                    "VM API socket could not be attached".to_string(),
                ));
            }
        };

        let micro_vm = machine.start_instance().await.map_err(sdk_err)?;
        instance.phase = FcPhase::Live {
            micro_vm,
            state: LiveState::Running,
            forking: ForkMemory::new(tracks_dirty_pages(&instance.proto_config)),
        };
        info!("FC: VM {} started successfully", vm_id);
        Ok(())
    }

    async fn stop_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Stopping VM {}", vm_id);

        // A running guest with the agent can shut itself down. Firecracker has
        // no power button, and Ctrl+Alt+Del needs an i8042 driver the guest
        // kernel doesn't have.
        let agent_socket = {
            let instance = self.lock_vm(vm_id).await?;
            match instance.phase {
                FcPhase::Live {
                    state: LiveState::Running,
                    ..
                } => Self::vsock_socket_path_from_config(&instance.proto_config.vsock),
                _ => None,
            }
        };
        let guest_shut_down = match agent_socket {
            Some(socket) => self.shut_down_guest(vm_id, &socket).await,
            None => false,
        };

        let mut instance = self.lock_vm(vm_id).await?;
        if !guest_shut_down {
            match instance.process.take() {
                Some(mut process) => {
                    let _ = process.kill().await;
                }
                None if matches!(instance.phase, FcPhase::Live { .. }) => warn!(
                    "FC: VM {} was recovered after a node restart, so its process \
                     can't be killed; it may still be running",
                    vm_id
                ),
                None => {}
            }
        }
        instance.phase = FcPhase::Stopped;
        info!("FC: VM {} stopped", vm_id);
        Ok(())
    }

    async fn force_stop_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Force stopping VM {}", vm_id);

        let mut instance = self.lock_vm(vm_id).await?;

        if let Some(mut process) = instance.process.take()
            && let Err(e) = process.kill().await
        {
            warn!("FC: Failed to kill process for VM {}: {}", vm_id, e);
        }

        instance.phase = FcPhase::Stopped;
        info!("FC: VM {} force stopped", vm_id);
        Ok(())
    }

    async fn pause_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Pausing VM {}", vm_id);

        let mut instance = self.lock_vm(vm_id).await?;

        let (micro_vm, state) = instance.live("VM is not running")?;
        if *state == LiveState::Paused {
            info!("FC: VM {} already paused, skipping", vm_id);
            return Ok(());
        }

        micro_vm
            .patch_vm(&Vm::new(FcVmState::Paused))
            .await
            .map_err(sdk_err)?;

        *state = LiveState::Paused;
        info!("FC: VM {} paused", vm_id);
        Ok(())
    }

    async fn resume_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Resuming VM {}", vm_id);

        let mut instance = self.lock_vm(vm_id).await?;

        let (micro_vm, state) = instance.live("VM is not paused")?;

        micro_vm
            .patch_vm(&Vm::new(FcVmState::Resumed))
            .await
            .map_err(sdk_err)?;

        *state = LiveState::Running;
        info!("FC: VM {} resumed", vm_id);
        Ok(())
    }

    async fn delete_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Deleting VM {}", vm_id);

        let mut instance = self
            .remove_vm(vm_id)
            .await
            .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?;

        if let Some(mut process) = instance.process.take()
            && let Err(e) = process.kill().await
        {
            warn!("FC: Failed to kill process for VM {}: {}", vm_id, e);
        }

        if instance.socket_path.exists() {
            let _ = tokio::fs::remove_file(&instance.socket_path).await;
        }

        let config_path = self.config_path(vm_id);
        if config_path.exists() {
            let _ = tokio::fs::remove_file(&config_path).await;
        }

        let seed_path = self.cloud_init_seed_path(vm_id);
        if tokio::fs::try_exists(&seed_path).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&seed_path).await;
        }

        if let Some(vsock_socket_path) =
            Self::vsock_socket_path_from_config(&instance.proto_config.vsock)
            && vsock_socket_path.exists()
        {
            let _ = tokio::fs::remove_file(vsock_socket_path).await;
        }

        for tap in &instance.tap_devices {
            Self::delete_tap_device(tap).await;
        }

        self.unmap_oci_disk(vm_id).await;

        info!("FC: VM {} deleted", vm_id);
        Ok(())
    }

    async fn get_vm_info(&self, vm_id: &str) -> Result<VmState, VmmError> {
        let mut instance = self.lock_vm(vm_id).await?;

        // Only a live VM is asked: a stopped one recovered after a node
        // restart may still answer on its socket.
        if let FcPhase::Live {
            micro_vm, state, ..
        } = &mut instance.phase
            && let Ok(info) = micro_vm.describe_instance().await
        {
            *state = info.state.into();
        }

        Ok(instance.to_vm_state())
    }

    async fn list_vms(&self) -> Vec<VmState> {
        self.vms
            .lock()
            .unwrap()
            .values()
            .map(ManagedVm::to_vm_state)
            .collect()
    }

    async fn snapshot_vm(&self, vm_id: &str, destination_url: &str) -> Result<(), VmmError> {
        info!("FC: Snapshotting VM {} to {}", vm_id, destination_url);

        // A snapshot resets the dirty-page log a fork diffs against: wait for
        // a fork round of the VM to end, and forget its base.
        let forking = match &self.lock_vm(vm_id).await?.phase {
            FcPhase::Live { forking, .. } => Some(forking.clone()),
            _ => None,
        };
        if let Some(forking) = forking {
            forking.lock().await.forget_base();
        }

        let mut instance = self.lock_vm(vm_id).await?;

        let (micro_vm, state) = instance.live("VM is not running")?;
        // Only pause if not already paused — FC returns 400 on a redundant pause.
        if *state != LiveState::Paused {
            micro_vm
                .patch_vm(&Vm::new(FcVmState::Paused))
                .await
                .map_err(sdk_err)?;
            *state = LiveState::Paused;
        }

        let dest = destination_url
            .strip_prefix("file://")
            .unwrap_or(destination_url);

        tokio::fs::create_dir_all(dest).await.map_err(|e| {
            VmmError::ProcessError(format!(
                "Failed to create snapshot directory {}: {}",
                dest, e
            ))
        })?;

        let mem_path = format!("{}/mem.snap", dest);
        let state_path = format!("{}/vm.snap", dest);

        let (micro_vm, _) = instance.live("VM is not running")?;

        micro_vm
            .create_snapshot(&SnapshotCreateParams::new(mem_path, state_path))
            .await
            .map_err(sdk_err)?;

        info!("FC: VM {} snapshotted to {}", vm_id, destination_url);
        Ok(())
    }

    async fn restore_vm(&self, vm_id: &str, source_url: &str) -> Result<(), VmmError> {
        info!("FC: Restoring VM {} from {}", vm_id, source_url);

        // Clean up any existing instance.
        if let Some(mut instance) = self.remove_vm(vm_id).await {
            if let Some(mut process) = instance.process.take() {
                let _ = process.kill().await;
            }
            if instance.socket_path.exists() {
                let _ = tokio::fs::remove_file(&instance.socket_path).await;
            }
        }

        tokio::fs::create_dir_all(&self.runtime_dir)
            .await
            .map_err(VmmError::SpawnError)?;

        let socket_path = self.socket_path(vm_id);
        let log_path = self.log_path(vm_id);

        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }

        let log_file = tokio::fs::File::create(&log_path)
            .await
            .map_err(VmmError::SpawnError)?
            .into_std()
            .await;
        let stderr_file = log_file.try_clone().map_err(VmmError::SpawnError)?;

        let process = Command::new(&self.fc_binary)
            .arg("--api-sock")
            .arg(&socket_path)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .kill_on_drop(true)
            .spawn()
            .map_err(VmmError::SpawnError)?;

        let src = source_url.trim_prefix("file://");
        let mem_path = format!("{}/mem.snap", src);
        let state_path = format!("{}/vm.snap", src);

        let params = SnapshotLoadParams {
            snapshot_path: state_path,
            mem_backend: Some(Box::new(MemoryBackend::new(BackendType::File, mem_path))),
            resume_vm: Some(true),
            ..Default::default()
        };

        // Attach to the fresh process and load the snapshot.
        let machine = Machine::attach(&socket_path).await.map_err(sdk_err)?;
        let micro_vm = machine.load_and_resume(&params).await.map_err(sdk_err)?;

        // Reload persisted proto config for gRPC state reporting.
        let proto_config =
            self.load_persisted_config(vm_id)
                .await?
                .unwrap_or_else(|| ProtoVmConfig {
                    vm_id: vm_id.to_string(),
                    ..Default::default()
                });

        let instance = FcVmInstance::new(
            proto_config,
            Some(process),
            socket_path,
            vec![],
            // Loaded without dirty-page tracking.
            FcPhase::Live {
                micro_vm,
                state: LiveState::Running,
                forking: ForkMemory::new(false),
            },
        );

        self.vms
            .lock()
            .unwrap()
            .insert(vm_id.to_string(), ManagedVm::new(instance));
        info!("FC: VM {} restored successfully from {}", vm_id, source_url);
        Ok(())
    }

    async fn exec_vm(
        &self,
        vm_id: &str,
        command: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<ExecVmResponse, VmmError> {
        FirecrackerManager::exec_vm(self, vm_id, command, timeout_secs).await
    }

    async fn guest_agent_socket(&self, vm_id: &str) -> Result<PathBuf, VmmError> {
        FirecrackerManager::guest_agent_socket(self, vm_id).await
    }

    async fn fork_vm(&self, source_id: &str, forks: &[Fork]) -> Result<(), VmmError> {
        self.fork(source_id, forks).await
    }

    async fn resume_forks(&self, source_id: &str, fork_ids: &[String]) -> Result<(), VmmError> {
        FirecrackerManager::resume_forks(self, source_id, fork_ids).await
    }

    async fn recover_vms(&self) {
        info!(
            "FC: Scanning for surviving Firecracker processes in {:?}",
            self.runtime_dir
        );

        let mut read_dir = match tokio::fs::read_dir(&self.runtime_dir).await {
            Ok(rd) => rd,
            Err(e) => {
                warn!("FC: Failed to read runtime dir for recovery: {}", e);
                return;
            }
        };

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            // A fork base of the node's last run: a recovered VM's next fork
            // takes a full snapshot.
            if name.contains(".fork-base-") {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    warn!(
                        "FC: Failed to remove stale fork base {}: {}",
                        path.display(),
                        e
                    );
                }
                continue;
            }
            if !name.ends_with(".fc.sock") {
                continue;
            }
            let vm_id = name.trim_end_matches(".fc.sock").to_string();

            let proto_config = match self.load_persisted_config(&vm_id).await {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => {
                    warn!("FC: Failed to load config for VM {}: {}", vm_id, e);
                    continue;
                }
            };

            let tap_devices: Vec<String> = proto_config
                .networks
                .iter()
                .filter_map(|n| n.tap.clone())
                .filter(|t| t.starts_with("qf"))
                .collect();

            // Attach to the socket and check live status.
            let phase = match Machine::attach(&path).await {
                Ok(machine) => {
                    let mut micro_vm = machine.assume_running();
                    let state = match micro_vm.describe_instance().await {
                        Ok(info) => info.state.into(),
                        Err(_) => LiveState::Unknown,
                    };
                    // Whether it tracks dirty pages isn't known.
                    FcPhase::Live {
                        micro_vm,
                        state,
                        forking: ForkMemory::new(false),
                    }
                }
                Err(e) => {
                    warn!("FC: Failed to attach to recovered VM {}: {}", vm_id, e);
                    FcPhase::Detached
                }
            };

            let instance = FcVmInstance::new(proto_config, None, path.clone(), tap_devices, phase);

            if let Some(cid) = instance.proto_config.vsock.as_ref().and_then(|v| v.cid) {
                Self::advance_vsock_cid_past(cid);
            }

            let status = instance.status();
            self.vms
                .lock()
                .unwrap()
                .insert(vm_id.clone(), ManagedVm::new(instance));
            info!("FC: Recovered VM {} with status {:?}", vm_id, status);
        }
    }

    fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    fn api_socket(&self, vm_id: &str) -> PathBuf {
        self.socket_path(vm_id)
    }

    async fn is_vm_process_alive(&self, vm_id: &str) -> bool {
        let Ok(mut instance) = self.lock_vm(vm_id).await else {
            return false;
        };
        if let FcPhase::Stopped = instance.phase {
            return false;
        }
        match &mut instance.process {
            Some(child) => child.try_wait().ok().flatten().is_none(),
            // Recovered at startup: qarax holds no handle to its process.
            None => instance.socket_path.exists(),
        }
    }
}
