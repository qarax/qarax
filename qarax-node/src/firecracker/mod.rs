//! Firecracker VMM backend.
//!
//! Implements `VmmManager` for Firecracker microVMs using the
//! `firecracker-rust-sdk` crate, which provides typed models and an HTTP/1.1
//! client over the Firecracker Unix API socket.
//!
//! Lifecycle mapping:
//!   create_vm   — spawn process + configure via SDK (Machine)
//!   start_vm    — Machine::start_instance() → MicroVm
//!   stop_vm     — MicroVm::send_ctrl_alt_del()
//!   force_stop  — kill the process
//!   pause_vm    — MicroVm::patch_vm(Paused)
//!   resume_vm   — MicroVm::patch_vm(Resumed)
//!   delete_vm   — kill + cleanup
//!   snapshot_vm — MicroVm::create_snapshot()
//!   restore_vm  — spawn + Machine::load_and_resume()

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use firecracker_rust_sdk::machine::{Machine, MicroVm};
use firecracker_rust_sdk::models::{
    BootSource, Drive, MachineConfiguration, MemoryBackend, NetworkInterface, SnapshotCreateParams,
    SnapshotLoadParams, Vm, Vsock, instance_info::State as FcInstanceState,
    memory_backend::BackendType, vm::State as FcVmState,
};
use prost::Message;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::rpc::node::ExecVmResponse;
use crate::rpc::node::{VmConfig as ProtoVmConfig, VmState, VmStatus};
use crate::vmm::{VmmError, VmmManager};

mod exec;
mod helpers;

/// Tracks the SDK client state for a single Firecracker VM.
enum FcApiClient {
    /// FC process is up but `InstanceStart` has not been sent yet.
    PreBoot(Machine<'static>),
    /// VM is running (or paused/snapshot-restoring).
    Running(MicroVm<'static>),
}

struct FcVmInstance {
    proto_config: ProtoVmConfig,
    process: Option<Child>,
    socket_path: PathBuf,
    status: VmStatus,
    tap_devices: Vec<String>,
    client: Option<FcApiClient>,
}

impl FcVmInstance {
    fn to_vm_state(&self) -> VmState {
        VmState {
            config: Some(self.proto_config.clone()),
            status: self.status.into(),
            memory_actual_size: None,
        }
    }
}

pub struct FirecrackerManager {
    runtime_dir: PathBuf,
    fc_binary: PathBuf,
    vms: Arc<Mutex<HashMap<String, FcVmInstance>>>,
}

impl FirecrackerManager {
    pub fn new(runtime_dir: impl Into<PathBuf>, fc_binary: impl Into<PathBuf>) -> Self {
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
            vms: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn fc_binary(&self) -> &Path {
        &self.fc_binary
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

        machine
            .put_machine_config(&MachineConfiguration::new(mem_mib, cpus))
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
                machine.put_drive(&disk.id, &drive).await.map_err(sdk_err)?;
                debug!("FC drive {} configured (root={})", disk.id, is_root);
            }
        }

        for net in &config.networks {
            if let Some(tap) = &net.tap {
                let mut iface = NetworkInterface::new(tap.clone(), net.id.clone());
                iface.guest_mac = net.mac.clone();
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
}

fn sdk_err(e: firecracker_rust_sdk::error::Error) -> VmmError {
    VmmError::ProcessError(e.to_string())
}

#[async_trait::async_trait]
impl VmmManager for FirecrackerManager {
    async fn create_vm(&self, config: ProtoVmConfig) -> Result<VmState, VmmError> {
        let vm_id = config.vm_id.clone();
        info!("FC: Creating VM {}", vm_id);

        {
            let vms = self.vms.lock().await;
            if vms.contains_key(&vm_id) {
                return Err(VmmError::VmAlreadyExists(vm_id));
            }
        }

        tokio::fs::create_dir_all(&self.runtime_dir)
            .await
            .map_err(VmmError::SpawnError)?;

        let mut config = config;

        // Create TAP devices for network interfaces that don't have one yet.
        let mut tap_devices: Vec<String> = Vec::new();
        for (i, net) in config.networks.iter_mut().enumerate() {
            if net.tap.is_none() && !net.vhost_user.unwrap_or(false) {
                let tap_name = Self::tap_name_for_net(&vm_id, i);
                if let Err(e) = Self::create_tap_device(&tap_name).await {
                    for tap in &tap_devices {
                        Self::delete_tap_device(tap).await;
                    }
                    return Err(e);
                }
                net.tap = Some(tap_name.clone());
                tap_devices.push(tap_name);
            }
        }

        // Attach TAPs to bridges if specified.
        for net in &config.networks {
            if let (Some(tap_name), Some(bridge_name)) = (&net.tap, &net.bridge)
                && let Err(e) =
                    crate::networking::bridge::attach_to_bridge(tap_name, bridge_name).await
            {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                return Err(VmmError::TapError(format!(
                    "Failed to attach TAP {} to bridge {}: {}",
                    tap_name, bridge_name, e
                )));
            }
        }

        if let Some(vsock) = config.vsock.as_mut() {
            self.resolve_vsock_config(&vm_id, vsock);
        }
        let vsock_socket_path = Self::vsock_socket_path_from_config(&config.vsock);
        if let Some(path) = &vsock_socket_path
            && path.exists()
        {
            let _ = tokio::fs::remove_file(path).await;
        }

        // Build cloud-init seed image if configured.
        if let Some(ci) = &config.cloud_init
            && !ci.user_data.is_empty()
        {
            let seed_path = self.cloud_init_seed_path(&vm_id);
            let network_config =
                (!ci.network_config.is_empty()).then_some(ci.network_config.as_str());
            let buf =
                crate::cloud_init::build_seed_image(&ci.user_data, &ci.meta_data, network_config)
                    .map_err(|e| VmmError::InvalidConfig(e.to_string()))?;
            tokio::fs::write(&seed_path, buf)
                .await
                .map_err(VmmError::SpawnError)?;
            config.disks.push(crate::rpc::node::DiskConfig {
                id: "cidata".to_string(),
                path: Some(seed_path.display().to_string()),
                readonly: Some(true),
                ..Default::default()
            });
            info!("FC: cloud-init seed disk attached for VM {}", vm_id);
        }

        let socket_path = self.socket_path(&vm_id);
        let log_path = self.log_path(&vm_id);
        let config_path = self.config_path(&vm_id);

        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }

        let log_file = tokio::fs::File::create(&log_path)
            .await
            .map_err(VmmError::SpawnError)?
            .into_std()
            .await;
        let stderr_file = log_file.try_clone().map_err(VmmError::SpawnError)?;

        let process = match Command::new(&self.fc_binary)
            .arg("--api-sock")
            .arg(&socket_path)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .kill_on_drop(true)
            .spawn()
        {
            Ok(p) => p,
            Err(e) => {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                return Err(VmmError::SpawnError(e));
            }
        };

        info!("FC: process started with PID {:?}", process.id());

        // Attach SDK client — Machine::attach waits for the socket to be ready.
        let mut machine = match Machine::attach(&socket_path).await {
            Ok(m) => m,
            Err(e) => {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                return Err(sdk_err(e));
            }
        };

        // Configure machine, boot source, drives, networks.
        if let Err(e) = Self::configure_vm(&mut machine, &socket_path, &config).await {
            for tap in &tap_devices {
                Self::delete_tap_device(tap).await;
            }
            return Err(e);
        }

        // Persist config for recovery.
        let config_bytes = config.encode_to_vec();
        if let Err(e) = tokio::fs::write(&config_path, config_bytes).await {
            warn!("FC: Failed to persist config for VM {}: {}", vm_id, e);
        }

        let instance = FcVmInstance {
            proto_config: config.clone(),
            process: Some(process),
            socket_path,
            status: VmStatus::Created,
            tap_devices,
            client: Some(FcApiClient::PreBoot(machine)),
        };

        let state = instance.to_vm_state();
        self.vms.lock().await.insert(vm_id.clone(), instance);
        info!("FC: VM {} created successfully", vm_id);
        Ok(state)
    }

    async fn start_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Starting VM {}", vm_id);

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?;

        let client = instance
            .client
            .take()
            .ok_or_else(|| VmmError::ProcessError("no client for VM".to_string()))?;

        let FcApiClient::PreBoot(machine) = client else {
            instance.client = Some(client);
            return Err(VmmError::ProcessError("VM already started".to_string()));
        };

        let micro_vm = machine.start_instance().await.map_err(sdk_err)?;
        instance.client = Some(FcApiClient::Running(micro_vm));
        instance.status = VmStatus::Running;
        info!("FC: VM {} started successfully", vm_id);
        Ok(())
    }

    async fn stop_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Stopping VM {}", vm_id);

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?;

        if let Some(FcApiClient::Running(ref mut micro_vm)) = instance.client {
            match micro_vm.send_ctrl_alt_del().await {
                Ok(_) => {}
                Err(e) => {
                    warn!(
                        "FC: VM {} soft-stop failed (treating as stopped): {}",
                        vm_id, e
                    );
                }
            }
        }

        instance.status = VmStatus::Shutdown;
        info!("FC: VM {} stopped", vm_id);
        Ok(())
    }

    async fn force_stop_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Force stopping VM {}", vm_id);

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?;

        if let Some(mut process) = instance.process.take()
            && let Err(e) = process.kill().await
        {
            warn!("FC: Failed to kill process for VM {}: {}", vm_id, e);
        }

        instance.status = VmStatus::Shutdown;
        info!("FC: VM {} force stopped", vm_id);
        Ok(())
    }

    async fn pause_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Pausing VM {}", vm_id);

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?;

        if instance.status == VmStatus::Paused {
            info!("FC: VM {} already paused, skipping", vm_id);
            return Ok(());
        }

        let Some(FcApiClient::Running(ref mut micro_vm)) = instance.client else {
            return Err(VmmError::ProcessError("VM is not running".to_string()));
        };

        micro_vm
            .patch_vm(&Vm::new(FcVmState::Paused))
            .await
            .map_err(sdk_err)?;

        instance.status = VmStatus::Paused;
        info!("FC: VM {} paused", vm_id);
        Ok(())
    }

    async fn resume_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Resuming VM {}", vm_id);

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?;

        let Some(FcApiClient::Running(ref mut micro_vm)) = instance.client else {
            return Err(VmmError::ProcessError("VM is not paused".to_string()));
        };

        micro_vm
            .patch_vm(&Vm::new(FcVmState::Resumed))
            .await
            .map_err(sdk_err)?;

        instance.status = VmStatus::Running;
        info!("FC: VM {} resumed", vm_id);
        Ok(())
    }

    async fn delete_vm(&self, vm_id: &str) -> Result<(), VmmError> {
        info!("FC: Deleting VM {}", vm_id);

        let mut instance = {
            let mut vms = self.vms.lock().await;
            vms.remove(vm_id)
                .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?
        };

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

        info!("FC: VM {} deleted", vm_id);
        Ok(())
    }

    async fn get_vm_info(&self, vm_id: &str) -> Result<VmState, VmmError> {
        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?;

        // Trust explicit Shutdown — the FC process may still be responding on
        // the socket briefly after stop_vm (e.g. paused VM sent Ctrl+Alt+Del).
        if instance.status == VmStatus::Shutdown {
            return Ok(instance.to_vm_state());
        }

        if let Some(FcApiClient::Running(ref mut micro_vm)) = instance.client
            && let Ok(info) = micro_vm.describe_instance().await
        {
            instance.status = match info.state {
                FcInstanceState::Running => VmStatus::Running,
                FcInstanceState::Paused => VmStatus::Paused,
                FcInstanceState::NotStarted => VmStatus::Created,
            };
        }

        Ok(instance.to_vm_state())
    }

    async fn list_vms(&self) -> Vec<VmState> {
        let vms = self.vms.lock().await;
        vms.values().map(|i| i.to_vm_state()).collect()
    }

    async fn snapshot_vm(&self, vm_id: &str, destination_url: &str) -> Result<(), VmmError> {
        info!("FC: Snapshotting VM {} to {}", vm_id, destination_url);

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?;

        // Only pause if not already paused — FC returns 400 on a redundant pause.
        if instance.status != VmStatus::Paused {
            let Some(FcApiClient::Running(ref mut micro_vm)) = instance.client else {
                return Err(VmmError::ProcessError("VM is not running".to_string()));
            };
            micro_vm
                .patch_vm(&Vm::new(FcVmState::Paused))
                .await
                .map_err(sdk_err)?;
            instance.status = VmStatus::Paused;
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

        let Some(FcApiClient::Running(ref mut micro_vm)) = instance.client else {
            return Err(VmmError::ProcessError("VM is not running".to_string()));
        };

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
        {
            let mut vms = self.vms.lock().await;
            if let Some(mut instance) = vms.remove(vm_id) {
                if let Some(mut process) = instance.process.take() {
                    let _ = process.kill().await;
                }
                if instance.socket_path.exists() {
                    let _ = tokio::fs::remove_file(&instance.socket_path).await;
                }
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

        let src = source_url.strip_prefix("file://").unwrap_or(source_url);
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

        let instance = FcVmInstance {
            proto_config,
            process: Some(process),
            socket_path,
            status: VmStatus::Running,
            tap_devices: vec![],
            client: Some(FcApiClient::Running(micro_vm)),
        };

        self.vms.lock().await.insert(vm_id.to_string(), instance);
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
            let (status, client) = match Machine::attach(&path).await {
                Ok(machine) => {
                    let mut micro_vm = machine.assume_running();
                    let status = match micro_vm.describe_instance().await {
                        Ok(info) => match info.state {
                            FcInstanceState::Running => VmStatus::Running,
                            FcInstanceState::Paused => VmStatus::Paused,
                            FcInstanceState::NotStarted => VmStatus::Unknown,
                        },
                        Err(_) => VmStatus::Unknown,
                    };
                    (status, Some(FcApiClient::Running(micro_vm)))
                }
                Err(e) => {
                    warn!("FC: Failed to attach to recovered VM {}: {}", vm_id, e);
                    (VmStatus::Unknown, None)
                }
            };

            let instance = FcVmInstance {
                proto_config,
                process: None,
                socket_path: path.clone(),
                status,
                tap_devices,
                client,
            };

            if let Some(cid) = instance.proto_config.vsock.as_ref().and_then(|v| v.cid) {
                Self::advance_vsock_cid_past(cid);
            }

            let mut vms = self.vms.lock().await;
            vms.insert(vm_id.clone(), instance);
            info!("FC: Recovered VM {} with status {:?}", vm_id, status);
        }
    }

    fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    async fn is_vm_process_alive(&self, vm_id: &str) -> bool {
        let mut vms = self.vms.lock().await;
        let Some(instance) = vms.get_mut(vm_id) else {
            return false;
        };
        match &mut instance.process {
            Some(child) => child.try_wait().ok().flatten().is_none(),
            None => instance.socket_path.exists(),
        }
    }
}
