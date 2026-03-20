//! VM Manager for Cloud Hypervisor processes
//!
//! Manages the lifecycle of Cloud Hypervisor processes using the SDK.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use bytes::Bytes;
use cloud_hypervisor_sdk::client::TokioIo;
use cloud_hypervisor_sdk::machine::{Machine, MachineConfig, VM};
use cloud_hypervisor_sdk::models::{
    self, CpusConfig, FsConfig, MemoryConfig, PayloadConfig, VmConfig,
    console_config::Mode as ConsoleMode,
};
use futures::stream::StreamExt;
use http_body_util::{Empty, Full, combinators::BoxBody};
use hyper::Request;
use tokio::net::UnixStream;

use prost::Message;

use crate::image_store::ImageStoreManager;
use crate::overlaybd::OverlayBdManager;
use crate::rpc::node::{
    ConsoleConfig as ProtoConsoleConfig, ConsoleMode as ProtoConsoleMode,
    CpuTopology as ProtoCpuTopology, CpusConfig as ProtoCpusConfig, DiskConfig as ProtoDiskConfig,
    FsConfig as ProtoFsConfig, MemoryConfig as ProtoMemoryConfig, NetConfig as ProtoNetConfig,
    PayloadConfig as ProtoPayloadConfig, RateLimiterConfig as ProtoRateLimiterConfig,
    RngConfig as ProtoRngConfig, TokenBucket as ProtoTokenBucket,
    VfioDeviceConfig as ProtoVfioDeviceConfig, VhostMode as ProtoVhostMode,
    VmConfig as ProtoVmConfig, VmState, VmStatus,
};

#[derive(Debug, Error)]
pub enum VmManagerError {
    #[error("VM {0} not found")]
    VmNotFound(String),

    #[error("VM {0} already exists")]
    VmAlreadyExists(String),

    #[error("Failed to spawn Cloud Hypervisor: {0}")]
    SpawnError(std::io::Error),

    #[error("Cloud Hypervisor SDK error: {0}")]
    SdkError(#[from] cloud_hypervisor_sdk::error::Error),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("TAP device error: {0}")]
    TapError(String),

    #[error("OverlayBD error: {0}")]
    OverlayBdError(#[from] crate::overlaybd::OverlayBdError),

    #[error("Migration error: {0}")]
    MigrationError(String),
}

/// Represents a running VM instance
struct VmInstance {
    /// The VM configuration (proto format)
    proto_config: ProtoVmConfig,
    /// Cloud Hypervisor process (None for recovered VMs)
    process: Option<Child>,
    /// SDK VM handle
    vm: VM<'static>,
    /// Path to the API socket
    socket_path: PathBuf,
    /// Current status
    status: VmStatus,
    /// TAP devices created by qarax-node for this VM (cleaned up on delete)
    tap_devices: Vec<String>,
    /// passt backend processes started by qarax-node for this VM
    passt_processes: Vec<Child>,
    /// PTY path for serial console (if PTY mode is enabled)
    serial_pty_path: Option<String>,
    /// PTY path for console device (if PTY mode is enabled)
    console_pty_path: Option<String>,
    /// Whether this VM uses an OverlayBD block device (needs cleanup on delete)
    has_overlaybd: bool,
}

impl VmInstance {
    fn to_vm_state(&self) -> VmState {
        VmState {
            config: Some(self.proto_config.clone()),
            status: self.status.into(),
            memory_actual_size: None,
        }
    }
}

/// Manager for Cloud Hypervisor VM instances
pub struct VmManager {
    /// Base directory for VM runtime files (sockets, logs, etc.)
    runtime_dir: PathBuf,
    /// Path to cloud-hypervisor binary
    ch_binary: PathBuf,
    /// Running VM instances
    vms: Arc<Mutex<HashMap<String, VmInstance>>>,
    /// Optional image store manager for OCI image boot support (virtiofs path)
    image_store_manager: Option<Arc<ImageStoreManager>>,
    /// Optional OverlayBD manager for lazy block-level OCI image boot
    overlaybd_manager: Option<Arc<OverlayBdManager>>,
    /// Path to the qarax-init binary (injected into OverlayBD-backed VMs)
    qarax_init_binary: Option<PathBuf>,
}

impl VmManager {
    /// Create a new VM manager
    pub fn new(
        runtime_dir: impl Into<PathBuf>,
        ch_binary: impl Into<PathBuf>,
        image_store_manager: Option<Arc<ImageStoreManager>>,
    ) -> Self {
        Self::with_overlaybd(runtime_dir, ch_binary, image_store_manager, None, None)
    }

    /// Create a new VM manager with optional OverlayBD support
    pub fn with_overlaybd(
        runtime_dir: impl Into<PathBuf>,
        ch_binary: impl Into<PathBuf>,
        image_store_manager: Option<Arc<ImageStoreManager>>,
        overlaybd_manager: Option<Arc<OverlayBdManager>>,
        qarax_init_binary: Option<PathBuf>,
    ) -> Self {
        let runtime_dir = runtime_dir.into();
        let ch_binary = ch_binary.into();

        info!(
            "VmManager initialized: runtime_dir={}, ch_binary={}",
            runtime_dir.display(),
            ch_binary.display()
        );

        Self {
            runtime_dir,
            ch_binary,
            vms: Arc::new(Mutex::new(HashMap::new())),
            image_store_manager,
            overlaybd_manager,
            qarax_init_binary,
        }
    }

    /// Get the image store manager if configured
    pub fn image_store_manager(&self) -> Option<&Arc<ImageStoreManager>> {
        self.image_store_manager.as_ref()
    }

    /// Get the OverlayBD manager if configured
    pub fn overlaybd_manager(&self) -> Option<&Arc<OverlayBdManager>> {
        self.overlaybd_manager.as_ref()
    }

    /// Get the runtime directory path
    pub fn runtime_dir(&self) -> &std::path::Path {
        &self.runtime_dir
    }

    /// Get the path to the Cloud Hypervisor binary
    pub fn ch_binary(&self) -> &std::path::Path {
        &self.ch_binary
    }

    /// Get the socket path for a VM
    fn socket_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.sock", vm_id))
    }

    fn cloud_init_seed_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}-cidata.img", vm_id))
    }

    /// Get the log path for a VM
    fn log_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.log", vm_id))
    }

    /// Get the config persistence path for a VM
    fn config_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.json", vm_id))
    }

    async fn load_persisted_vm_config(
        &self,
        vm_id: &str,
    ) -> Result<Option<ProtoVmConfig>, VmManagerError> {
        let config_path = self.config_path(vm_id);
        if !config_path.exists() {
            return Ok(None);
        }

        let config_bytes = tokio::fs::read(&config_path)
            .await
            .map_err(VmManagerError::SpawnError)?;

        let config = ProtoVmConfig::decode(config_bytes.as_slice()).map_err(|e| {
            VmManagerError::InvalidConfig(format!(
                "Failed to decode persisted config for VM {}: {}",
                vm_id, e
            ))
        })?;

        Ok(Some(config))
    }

    async fn ensure_vm_registered(&self, vm_id: &str) -> Result<(), VmManagerError> {
        {
            let vms = self.vms.lock().await;
            if vms.contains_key(vm_id) {
                return Ok(());
            }
        }

        let Some(config) = self.load_persisted_vm_config(vm_id).await? else {
            return Err(VmManagerError::VmNotFound(vm_id.to_string()));
        };

        info!(
            "VM {} missing from manager state; recreating from persisted config",
            vm_id
        );
        self.create_vm(config).await?;
        Ok(())
    }

    /// Scan for surviving Cloud Hypervisor processes and reconnect to them.
    /// Called on startup to recover VMs that survived a qarax-node restart.
    pub async fn recover_vms(&self) {
        info!(
            "Scanning for surviving VM processes in {:?}",
            self.runtime_dir
        );

        let mut read_dir = match tokio::fs::read_dir(&self.runtime_dir).await {
            Ok(rd) => rd,
            Err(e) => {
                warn!("Failed to read runtime dir for recovery: {}", e);
                return;
            }
        };

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("sock") {
                continue;
            }

            let vm_id = match path.file_stem().and_then(|s| s.to_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            // Load persisted proto config
            let proto_config = match self.load_persisted_vm_config(&vm_id).await {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => {
                    warn!("Failed to load config for VM {}: {}", vm_id, e);
                    continue;
                }
            };

            // Parse VM ID as UUID for SDK
            let vm_uuid = match Uuid::parse_str(&vm_id) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let socket_path = path.clone();
            let machine_config = MachineConfig {
                vm_id: vm_uuid,
                socket_path: Cow::Owned(socket_path.clone()),
                exec_path: Cow::Owned(self.ch_binary.clone()),
            };

            let mut vm = match Machine::connect(machine_config).await {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        "Failed to connect to VM {} socket (process may have died): {}",
                        vm_id, e
                    );
                    continue;
                }
            };

            // Get current status from Cloud Hypervisor
            let status = match vm.get_info().await {
                Ok(info) => match info.state {
                    models::vm_info::State::Created => VmStatus::Created,
                    models::vm_info::State::Running => VmStatus::Running,
                    models::vm_info::State::Paused => VmStatus::Paused,
                    models::vm_info::State::Shutdown => VmStatus::Shutdown,
                },
                Err(e) => {
                    warn!("Failed to get info for recovered VM {}: {}", vm_id, e);
                    VmStatus::Unknown
                }
            };

            // Re-derive managed TAP devices from the persisted config (tap names
            // were written into the config at create time).
            let tap_devices: Vec<String> = proto_config
                .networks
                .iter()
                .filter_map(|n| n.tap.clone())
                .filter(|t| t.starts_with("qt"))
                .collect();

            let instance = VmInstance {
                proto_config,
                process: None, // We don't have the child process handle for recovered VMs
                vm,
                socket_path,
                status,
                tap_devices,
                passt_processes: Vec::new(),
                serial_pty_path: None,
                console_pty_path: None,
                has_overlaybd: false, // Recovery doesn't restore OverlayBD state
            };

            let mut vms = self.vms.lock().await;
            vms.insert(vm_id.clone(), instance);
            info!("Recovered VM {} with status {:?}", vm_id, status);
        }
    }

    /// Extract the first 8 hex digits from a VM UUID string (dashes stripped).
    fn vm_hex_prefix(vm_id: &str) -> String {
        vm_id
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .take(8)
            .collect()
    }

    /// Generate a deterministic TAP device name for a network interface.
    ///
    /// Format: "qt" + first 8 hex chars of VM UUID + "n" + NIC index.
    /// Example: "qt24b6061en0" (12 chars, well within the 15-char Linux limit).
    fn tap_name_for_net(vm_id: &str, net_index: usize) -> String {
        format!("qt{}n{}", Self::vm_hex_prefix(vm_id), net_index)
    }

    fn passt_socket_path(&self, vm_id: &str, net_index: usize) -> PathBuf {
        self.runtime_dir.join(format!(
            "qp{}n{}.sock",
            Self::vm_hex_prefix(vm_id),
            net_index
        ))
    }

    fn should_spawn_passt(net: &ProtoNetConfig) -> bool {
        net.vhost_user.unwrap_or(false) && net.vhost_socket.as_deref() == Some("passt")
    }

    async fn start_passt_backend(socket_path: &Path) -> Result<Child, VmManagerError> {
        if socket_path.exists() {
            let _ = tokio::fs::remove_file(socket_path).await;
        }

        let mut child = Command::new("passt")
            .args(["--vhost-user", "--socket"])
            .arg(socket_path)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| VmManagerError::ProcessError(format!("failed to spawn passt: {e}")))?;

        for _ in 0..30 {
            if socket_path.exists() {
                info!("passt backend ready at {}", socket_path.display());
                return Ok(child);
            }
            sleep(Duration::from_millis(100)).await;
        }

        let _ = child.kill().await;
        Err(VmManagerError::ProcessError(format!(
            "passt socket not ready: {}",
            socket_path.display()
        )))
    }

    async fn cleanup_passt_processes(processes: &mut Vec<Child>) {
        for process in processes.iter_mut() {
            if let Err(e) = process.kill().await {
                warn!("Failed to kill passt process: {}", e);
            }
        }
        processes.clear();
    }

    /// Create a TAP device and bring it up.
    async fn create_tap_device(name: &str) -> Result<(), VmManagerError> {
        let add = Command::new("ip")
            .args(["tuntap", "add", name, "mode", "tap"])
            .status()
            .await
            .map_err(|e| VmManagerError::TapError(format!("failed to run ip tuntap add: {e}")))?;
        if !add.success() {
            return Err(VmManagerError::TapError(format!(
                "ip tuntap add {name} failed with status {add}"
            )));
        }

        let up = Command::new("ip")
            .args(["link", "set", name, "up"])
            .status()
            .await
            .map_err(|e| VmManagerError::TapError(format!("failed to run ip link set up: {e}")))?;
        if !up.success() {
            return Err(VmManagerError::TapError(format!(
                "ip link set {name} up failed with status {up}"
            )));
        }

        info!("TAP device {} created and up", name);
        Ok(())
    }

    /// Delete a TAP device. Logs a warning on failure but does not propagate errors.
    async fn delete_tap_device(name: &str) {
        match Command::new("ip")
            .args(["link", "delete", name])
            .status()
            .await
        {
            Ok(s) if s.success() => info!("TAP device {} deleted", name),
            Ok(s) => warn!("ip link delete {} failed with status {}", name, s),
            Err(e) => warn!("Failed to run ip link delete {}: {}", name, e),
        }
    }

    /// Create a new VM
    pub async fn create_vm(&self, config: ProtoVmConfig) -> Result<VmState, VmManagerError> {
        let vm_id = config.vm_id.clone();
        info!("Creating VM: {}", vm_id);

        // Check if VM already exists
        {
            let vms = self.vms.lock().await;
            if vms.contains_key(&vm_id) {
                return Err(VmManagerError::VmAlreadyExists(vm_id));
            }
        }

        // Create TAP devices for networks that need them, injecting the names
        // into the config so CH uses our managed devices (and we can clean them up).
        let mut config = config;
        let mut tap_devices: Vec<String> = Vec::new();
        let mut passt_processes: Vec<Child> = Vec::new();
        for (i, net) in config.networks.iter_mut().enumerate() {
            if Self::should_spawn_passt(net) {
                let socket_path = self.passt_socket_path(&vm_id, i);
                let passt = Self::start_passt_backend(&socket_path).await?;
                net.vhost_socket = Some(socket_path.to_string_lossy().to_string());
                passt_processes.push(passt);
                continue;
            }

            if !net.vhost_user.unwrap_or(false) && net.tap.is_none() {
                let tap_name = Self::tap_name_for_net(&vm_id, i);
                if let Err(e) = Self::create_tap_device(&tap_name).await {
                    for tap in &tap_devices {
                        Self::delete_tap_device(tap).await;
                    }
                    Self::cleanup_passt_processes(&mut passt_processes).await;
                    return Err(e);
                }
                net.tap = Some(tap_name.clone());
                tap_devices.push(tap_name);
            }
        }

        // Attach TAP devices to bridges if specified
        for net in config.networks.iter() {
            if let (Some(tap_name), Some(bridge_name)) = (&net.tap, &net.bridge)
                && let Err(e) =
                    crate::networking::bridge::attach_to_bridge(tap_name, bridge_name).await
            {
                tracing::error!(
                    "Failed to attach TAP {} to bridge {}: {}",
                    tap_name,
                    bridge_name,
                    e
                );
                // Clean up TAPs we created
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                Self::cleanup_passt_processes(&mut passt_processes).await;
                return Err(VmManagerError::TapError(format!(
                    "Failed to attach TAP {} to bridge {}: {}",
                    tap_name, bridge_name, e
                )));
            }
        }

        // Resolve OverlayBD disks: mount each disk that has oci_image_ref set and replace
        // the path with the resulting block device.
        let mut has_overlaybd = false;
        if let Some(obd_manager) = &self.overlaybd_manager {
            for disk in config.disks.iter_mut() {
                if let (Some(image_ref), Some(registry_url)) =
                    (disk.oci_image_ref.clone(), disk.registry_url.clone())
                {
                    let mounted = obd_manager.mount(&vm_id, &image_ref, &registry_url).await?;
                    let device_path = mounted.device_path.clone();
                    disk.path = Some(mounted.device_path);
                    disk.oci_image_ref = None;
                    disk.registry_url = None;
                    has_overlaybd = true;

                    // Inject qarax-init into the mounted block device so the VM
                    // boots with our init binary as PID 1.
                    if let Some(init_binary) = &self.qarax_init_binary {
                        obd_manager
                            .inject_init(
                                &vm_id,
                                &device_path,
                                &image_ref,
                                &registry_url,
                                init_binary,
                            )
                            .await?;
                    }
                }
            }
        } else {
            // No OverlayBD manager — log a warning if any disks request it
            for disk in &config.disks {
                if disk.oci_image_ref.is_some() {
                    warn!(
                        "Disk {} requests OverlayBD but no OverlayBD manager is configured",
                        disk.id
                    );
                }
            }
        }

        // If FsConfig entries have a bootstrap_path, start virtiofsd for each
        // and inject the socket path into the FsConfig.
        if !config.fs.is_empty() {
            if let Some(store) = &self.image_store_manager {
                for (i, fs) in config.fs.iter_mut().enumerate() {
                    if let Some(rootfs_path) = &fs.bootstrap_path {
                        let vm_fs_id = format!("{}-fs{}", vm_id, i);
                        match store
                            .start_virtiofsd(&vm_fs_id, std::path::Path::new(rootfs_path))
                            .await
                        {
                            Ok(socket_path) => {
                                fs.socket = socket_path.to_string_lossy().to_string();
                                info!(
                                    "virtiofsd started for VM {} fs{} at {}",
                                    vm_id, i, fs.socket
                                );
                            }
                            Err(e) => {
                                warn!("Failed to start virtiofsd for VM {} fs{}: {}", vm_id, i, e);
                            }
                        }
                    }
                }
            } else {
                debug!(
                    "FsConfig entries present but no ImageStoreManager configured — skipping virtiofsd startup"
                );
            }
        }

        // Generate a cloud-init NoCloud seed image and attach it as a read-only
        // disk if the VM has cloud-init data configured.
        if let Some(ci) = &config.cloud_init
            && !ci.user_data.is_empty()
        {
            let seed_path = self.cloud_init_seed_path(&vm_id);
            // runtime_dir is created unconditionally below; seed_path lives there.
            let network_config =
                (!ci.network_config.is_empty()).then_some(ci.network_config.as_str());
            let buf =
                super::cloud_init::build_seed_image(&ci.user_data, &ci.meta_data, network_config)
                    .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;
            tokio::fs::write(&seed_path, buf)
                .await
                .map_err(VmManagerError::SpawnError)?;
            config.disks.push(ProtoDiskConfig {
                id: "cidata".to_string(),
                path: Some(seed_path.display().to_string()),
                readonly: Some(true),
                ..Default::default()
            });
            info!("Cloud-init seed disk attached for VM {}", vm_id);
        }

        // Ensure runtime directory exists
        tokio::fs::create_dir_all(&self.runtime_dir)
            .await
            .map_err(VmManagerError::SpawnError)?;

        let socket_path = self.socket_path(&vm_id);
        let log_path = self.log_path(&vm_id);
        let config_path = self.config_path(&vm_id);

        // Remove old socket if it exists
        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }

        // Spawn Cloud Hypervisor process directly
        debug!(
            "Spawning Cloud Hypervisor with socket: {}",
            socket_path.display()
        );

        let log_file = tokio::fs::File::create(&log_path)
            .await
            .map_err(VmManagerError::SpawnError)?
            .into_std()
            .await;
        let stderr_file = log_file.try_clone().map_err(VmManagerError::SpawnError)?;

        let process = match Command::new(&self.ch_binary)
            .arg("--api-socket")
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
                Self::cleanup_passt_processes(&mut passt_processes).await;
                return Err(VmManagerError::SpawnError(e));
            }
        };

        info!(
            "Cloud Hypervisor process started with PID: {:?}",
            process.id()
        );

        // Wait for socket to be available
        let max_retries = 50;
        let mut retries = 0;
        loop {
            match UnixStream::connect(&socket_path).await {
                Ok(_) => break,
                Err(_) if retries < max_retries => {
                    retries += 1;
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(e) => {
                    for tap in &tap_devices {
                        Self::delete_tap_device(tap).await;
                    }
                    Self::cleanup_passt_processes(&mut passt_processes).await;
                    return Err(VmManagerError::SpawnError(e));
                }
            }
        }

        // Validate kernel path before sending to CH
        if let Some(payload) = &config.payload {
            if let Some(kernel) = &payload.kernel {
                let kernel_path = std::path::Path::new(kernel);
                if kernel_path.exists() {
                    info!("Kernel path validated: {} (exists)", kernel);
                } else {
                    warn!("Kernel path does NOT exist: {}", kernel);
                }
            } else {
                warn!("No kernel path in payload config");
            }
            if let Some(initramfs) = &payload.initramfs {
                let initramfs_path = std::path::Path::new(initramfs);
                if initramfs_path.exists() {
                    info!("Initramfs path validated: {} (exists)", initramfs);
                } else {
                    warn!("Initramfs path does NOT exist: {}", initramfs);
                }
            }
        }

        // Convert proto config to SDK config
        let sdk_config = self.proto_to_sdk_config(&config)?;
        let json_config = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        info!("Creating VM with CH config: {}", json_config);

        // Send create request via raw API
        if let Err(e) =
            Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.create", Some(&json_config))
                .await
        {
            for tap in &tap_devices {
                Self::delete_tap_device(tap).await;
            }
            Self::cleanup_passt_processes(&mut passt_processes).await;
            return Err(e);
        }

        info!("VM {} created on Cloud Hypervisor", vm_id);

        // Query vm.info API to discover PTY paths.
        // Cloud Hypervisor exposes the allocated PTY device path in the
        // vm.info response (config.serial.file / config.console.file) after
        // vm.create completes.
        let (serial_pty, console_pty) = self.query_pty_paths(&socket_path, &config).await;

        // Persist proto config (as protobuf binary) for recovery after restart
        let config_bytes = config.encode_to_vec();
        if let Err(e) = tokio::fs::write(&config_path, config_bytes).await {
            warn!("Failed to persist config for VM {}: {}", vm_id, e);
        }

        // Parse VM ID as UUID for SDK
        let vm_uuid = Uuid::parse_str(&vm_id)
            .map_err(|e| VmManagerError::InvalidConfig(format!("Invalid VM ID: {}", e)))?;

        // Connect to the CH instance via SDK
        let machine_config = MachineConfig {
            vm_id: vm_uuid,
            socket_path: Cow::Owned(socket_path.clone()),
            exec_path: Cow::Owned(self.ch_binary.clone()),
        };

        let vm = match Machine::connect(machine_config).await {
            Ok(vm) => vm,
            Err(e) => {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                Self::cleanup_passt_processes(&mut passt_processes).await;
                return Err(e.into());
            }
        };

        let instance = VmInstance {
            proto_config: config.clone(),
            process: Some(process),
            vm,
            socket_path: socket_path.clone(),
            status: VmStatus::Created,
            tap_devices,
            passt_processes,
            serial_pty_path: serial_pty,
            console_pty_path: console_pty,
            has_overlaybd,
        };

        let state = instance.to_vm_state();

        {
            let mut vms = self.vms.lock().await;
            vms.insert(vm_id.clone(), instance);
        }

        info!("VM {} registered in manager", vm_id);
        Ok(state)
    }

    /// Start a VM
    pub async fn start_vm(&self, vm_id: &str) -> Result<(), VmManagerError> {
        info!("Starting VM: {}", vm_id);

        self.ensure_vm_registered(vm_id).await?;

        let (socket_path, proto_config) = {
            let mut vms = self.vms.lock().await;
            let instance = vms
                .get_mut(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

            (instance.socket_path.clone(), instance.proto_config.clone())
        };

        // Use raw API for boot so we get the full error response body
        Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.boot", None).await?;

        {
            let mut vms = self.vms.lock().await;
            if let Some(instance) = vms.get_mut(vm_id) {
                instance.status = VmStatus::Running;
            }
        }

        // Re-query PTY paths after boot in case they weren't available at create time.
        let (serial_pty, console_pty) = self.query_pty_paths(&socket_path, &proto_config).await;
        if serial_pty.is_some() || console_pty.is_some() {
            let mut vms = self.vms.lock().await;
            if let Some(instance) = vms.get_mut(vm_id) {
                if serial_pty.is_some() {
                    instance.serial_pty_path = serial_pty;
                }
                if console_pty.is_some() {
                    instance.console_pty_path = console_pty;
                }
            }
        }

        info!("VM {} started successfully", vm_id);
        Ok(())
    }

    /// Stop a VM
    pub async fn stop_vm(&self, vm_id: &str) -> Result<(), VmManagerError> {
        info!("Stopping VM: {}", vm_id);

        let socket_path = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
            instance.socket_path.clone()
        };

        // Best-effort: if CH is already gone (socket missing, connection refused),
        // log a warning and continue — the VM is effectively stopped.
        match Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.shutdown", None).await {
            Ok(_) => {}
            Err(e) => {
                warn!(
                    "VM {} CH shutdown request failed (treating as already stopped): {}",
                    vm_id, e
                );
            }
        }

        {
            let mut vms = self.vms.lock().await;
            if let Some(instance) = vms.get_mut(vm_id) {
                instance.status = VmStatus::Shutdown;
            }
        }

        info!("VM {} stopped successfully", vm_id);
        Ok(())
    }

    /// Pause a VM
    pub async fn pause_vm(&self, vm_id: &str) -> Result<(), VmManagerError> {
        info!("Pausing VM: {}", vm_id);

        let socket_path = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
            instance.socket_path.clone()
        };

        Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.pause", None).await?;

        let mut vms = self.vms.lock().await;
        if let Some(instance) = vms.get_mut(vm_id) {
            instance.status = VmStatus::Paused;
        }

        info!("VM {} paused successfully", vm_id);
        Ok(())
    }

    /// Resume a VM
    pub async fn resume_vm(&self, vm_id: &str) -> Result<(), VmManagerError> {
        info!("Resuming VM: {}", vm_id);

        let socket_path = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
            instance.socket_path.clone()
        };

        Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.resume", None).await?;

        let mut vms = self.vms.lock().await;
        if let Some(instance) = vms.get_mut(vm_id) {
            instance.status = VmStatus::Running;
        }

        info!("VM {} resumed successfully", vm_id);
        Ok(())
    }

    /// Snapshot a VM
    pub async fn snapshot_vm(&self, vm_id: &str, snapshot_url: &str) -> Result<(), VmManagerError> {
        info!("Snapshotting VM: {}", vm_id);
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
        let socket_path = instance.socket_path.clone();
        drop(vms);

        // Cloud Hypervisor requires the destination to be an existing directory.
        let dest_path = snapshot_url.strip_prefix("file://").unwrap_or(snapshot_url);
        tokio::fs::create_dir_all(dest_path).await.map_err(|e| {
            VmManagerError::ProcessError(format!(
                "Failed to create snapshot directory {}: {}",
                dest_path, e
            ))
        })?;

        let body = format!(r#"{{"destination_url":"{}"}}"#, snapshot_url);
        Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.snapshot", Some(&body)).await?;
        info!("VM {} snapshotted successfully to {}", vm_id, snapshot_url);
        Ok(())
    }

    /// Restore a VM from a snapshot.
    ///
    /// Spawns a fresh Cloud Hypervisor process for the given vm_id, then calls
    /// `PUT /api/v1/vm.restore` (without a preceding `vm.create`). Cloud Hypervisor
    /// reads all VM config from the snapshot, so no VmConfig is needed here.
    pub async fn restore_vm(&self, vm_id: &str, source_url: &str) -> Result<(), VmManagerError> {
        info!("Restoring VM {} from {}", vm_id, source_url);

        let config_path = self.config_path(vm_id);
        let proto_config = match tokio::fs::read(&config_path).await {
            Ok(config_bytes) => ProtoVmConfig::decode(config_bytes.as_slice()).map_err(|e| {
                VmManagerError::InvalidConfig(format!(
                    "Failed to decode persisted config for restored VM {}: {}",
                    vm_id, e
                ))
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProtoVmConfig {
                vm_id: vm_id.to_string(),
                serial: Some(ProtoConsoleConfig {
                    mode: ProtoConsoleMode::Pty as i32,
                    file: None,
                    socket: None,
                    iommu: None,
                }),
                ..Default::default()
            },
            Err(e) => return Err(VmManagerError::SpawnError(e)),
        };

        // Clean up any existing CH process for this vm_id.
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

        // Ensure runtime directory exists.
        tokio::fs::create_dir_all(&self.runtime_dir)
            .await
            .map_err(VmManagerError::SpawnError)?;

        let socket_path = self.socket_path(vm_id);
        let log_path = self.log_path(vm_id);

        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }

        let log_file = tokio::fs::File::create(&log_path)
            .await
            .map_err(VmManagerError::SpawnError)?
            .into_std()
            .await;
        let stderr_file = log_file.try_clone().map_err(VmManagerError::SpawnError)?;

        let process = Command::new(&self.ch_binary)
            .arg("--api-socket")
            .arg(&socket_path)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .kill_on_drop(true)
            .spawn()
            .map_err(VmManagerError::SpawnError)?;

        info!(
            "Cloud Hypervisor process for restore started with PID: {:?}",
            process.id()
        );

        // Wait for socket to be ready.
        let max_retries = 50;
        let mut retries = 0;
        loop {
            match UnixStream::connect(&socket_path).await {
                Ok(_) => break,
                Err(_) if retries < max_retries => {
                    retries += 1;
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(e) => return Err(VmManagerError::SpawnError(e)),
            }
        }

        // Call vm.restore — Cloud Hypervisor reads all config from the snapshot.
        // After vm.restore, CH leaves the VM in paused state; vm.resume is required.
        let body = format!(r#"{{"source_url":"{}","prefault":false}}"#, source_url);
        if let Err(e) =
            Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.restore", Some(&body)).await
        {
            // Kill the CH process if restore fails.
            let _ = tokio::fs::remove_file(&socket_path).await;
            return Err(e);
        }

        if let Err(e) = Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.resume", None).await
        {
            let _ = tokio::fs::remove_file(&socket_path).await;
            return Err(e);
        }

        let (serial_pty, console_pty) = self.query_pty_paths(&socket_path, &proto_config).await;

        // Register the restored instance with the persisted proto config so
        // console/PTY metadata remains available after snapshot restore.
        let vm_uuid = Uuid::parse_str(vm_id)
            .map_err(|e| VmManagerError::InvalidConfig(format!("Invalid VM ID: {}", e)))?;

        let machine_config = MachineConfig {
            vm_id: vm_uuid,
            socket_path: Cow::Owned(socket_path.clone()),
            exec_path: Cow::Owned(self.ch_binary.clone()),
        };

        let vm = Machine::connect(machine_config)
            .await
            .map_err(VmManagerError::SdkError)?;

        let instance = VmInstance {
            proto_config,
            process: Some(process),
            vm,
            socket_path: socket_path.clone(),
            status: VmStatus::Running,
            tap_devices: vec![],
            passt_processes: vec![],
            serial_pty_path: serial_pty,
            console_pty_path: console_pty,
            has_overlaybd: false,
        };

        {
            let mut vms = self.vms.lock().await;
            vms.insert(vm_id.to_string(), instance);
        }

        info!("VM {} restored successfully from {}", vm_id, source_url);
        Ok(())
    }

    /// Prepare this node to receive a live migration for the given VM.
    ///
    /// Steps:
    /// 1. Create TAP devices for all networks in the supplied config.
    /// 2. Spawn a Cloud Hypervisor process.
    /// 3. Call `vm.receive-migration` on that process.
    /// 4. Register a placeholder VmInstance so the VM is tracked.
    ///
    /// Returns the `receiver_url` that the source node must pass to
    /// `vm.send-migration` (e.g. `"tcp:0.0.0.0:49152"`).
    pub async fn receive_migration(
        &self,
        vm_id: &str,
        config: ProtoVmConfig,
        migration_port: u16,
    ) -> Result<String, VmManagerError> {
        info!(
            "Preparing to receive migration for VM {} on port {}",
            vm_id, migration_port
        );

        {
            let vms = self.vms.lock().await;
            if vms.contains_key(vm_id) {
                return Err(VmManagerError::VmAlreadyExists(vm_id.to_string()));
            }
        }

        // Pick a free TCP port if the caller passed 0.
        let port = if migration_port == 0 {
            tokio::net::TcpListener::bind("0.0.0.0:0")
                .await
                .map_err(|e| {
                    VmManagerError::MigrationError(format!("Failed to bind ephemeral port: {}", e))
                })?
                .local_addr()
                .map_err(|e| {
                    VmManagerError::MigrationError(format!("Failed to get ephemeral port: {}", e))
                })?
                .port()
        } else {
            migration_port
        };

        // Create TAP devices for the incoming VM's networks.
        let mut tap_devices: Vec<String> = Vec::new();
        let mut mutable_config = config.clone();
        for (i, net) in mutable_config.networks.iter_mut().enumerate() {
            if !net.vhost_user.unwrap_or(false) && net.tap.is_none() {
                let tap_name = Self::tap_name_for_net(vm_id, i);
                if let Err(e) = Self::create_tap_device(&tap_name).await {
                    for tap in &tap_devices {
                        Self::delete_tap_device(tap).await;
                    }
                    return Err(e);
                }
                // Attach to bridge if specified.
                if let Some(bridge_name) = &net.bridge
                    && let Err(e) =
                        crate::networking::bridge::attach_to_bridge(&tap_name, bridge_name).await
                {
                    for tap in &tap_devices {
                        Self::delete_tap_device(tap).await;
                    }
                    return Err(VmManagerError::TapError(format!(
                        "Failed to attach TAP {} to bridge {}: {}",
                        tap_name, bridge_name, e
                    )));
                }
                net.tap = Some(tap_name.clone());
                tap_devices.push(tap_name);
            }
        }

        // Ensure runtime directory exists.
        tokio::fs::create_dir_all(&self.runtime_dir)
            .await
            .map_err(VmManagerError::SpawnError)?;

        let socket_path = self.socket_path(vm_id);
        let log_path = self.log_path(vm_id);

        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }

        let log_file = match tokio::fs::File::create(&log_path).await {
            Ok(f) => f,
            Err(e) => {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                return Err(VmManagerError::SpawnError(e));
            }
        }
        .into_std()
        .await;

        let stderr_file = match log_file.try_clone() {
            Ok(f) => f,
            Err(e) => {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                return Err(VmManagerError::SpawnError(e));
            }
        };

        let process = Command::new(&self.ch_binary)
            .arg("--api-socket")
            .arg(&socket_path)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .kill_on_drop(true)
            .spawn()
            .map_err(VmManagerError::SpawnError)?;

        info!(
            "Cloud Hypervisor receive-migration process started with PID: {:?}",
            process.id()
        );

        // Wait for the API socket to become available.
        let max_retries = 50;
        let mut retries = 0;
        loop {
            match UnixStream::connect(&socket_path).await {
                Ok(_) => break,
                Err(_) if retries < max_retries => {
                    retries += 1;
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(e) => {
                    for tap in &tap_devices {
                        Self::delete_tap_device(tap).await;
                    }
                    return Err(VmManagerError::SpawnError(e));
                }
            }
        }

        let receiver_url = format!("tcp:0.0.0.0:{}", port);

        // Persist the config for recovery.
        let config_bytes = mutable_config.encode_to_vec();
        if let Err(e) = tokio::fs::write(self.config_path(vm_id), config_bytes).await {
            warn!("Failed to persist config for incoming VM {}: {}", vm_id, e);
        }

        let vm_uuid = Uuid::parse_str(vm_id)
            .map_err(|e| VmManagerError::InvalidConfig(format!("Invalid VM ID: {}", e)))?;

        let machine_config = MachineConfig {
            vm_id: vm_uuid,
            socket_path: Cow::Owned(socket_path.clone()),
            exec_path: Cow::Owned(self.ch_binary.clone()),
        };

        let vm = Machine::connect(machine_config)
            .await
            .map_err(VmManagerError::SdkError)?;

        let instance = VmInstance {
            proto_config: mutable_config,
            process: Some(process),
            vm,
            socket_path: socket_path.clone(),
            status: VmStatus::Created,
            tap_devices,
            passt_processes: Vec::new(),
            serial_pty_path: None,
            console_pty_path: None,
            has_overlaybd: false,
        };

        {
            let mut vms = self.vms.lock().await;
            vms.insert(vm_id.to_string(), instance);
        }

        // vm.receive-migration blocks until the sender completes the full transfer.
        // Spawn it as a background task so we can return the receiver URL immediately;
        // the control plane will call send_migration on the source concurrently.
        let body = format!(r#"{{"receiver_url":"{}"}}"#, receiver_url);
        let socket_path_bg = socket_path.clone();
        let vm_id_bg = vm_id.to_string();
        tokio::spawn(async move {
            match Self::send_api_request(
                &socket_path_bg,
                "PUT",
                "/api/v1/vm.receive-migration",
                Some(&body),
            )
            .await
            {
                Ok(_) => info!("VM {} receive-migration completed", vm_id_bg),
                Err(e) => error!(
                    "VM {} receive-migration background task failed: {}",
                    vm_id_bg, e
                ),
            }
        });

        info!(
            "VM {} ready to receive migration on {}",
            vm_id, receiver_url
        );
        Ok(receiver_url)
    }

    /// Send a live migration from this node to the destination.
    ///
    /// Calls `vm.send-migration` on the source Cloud Hypervisor process.
    /// This call blocks until Cloud Hypervisor completes the migration.
    /// On success the source VM process has exited; the VmInstance is removed
    /// from the manager (TAP cleanup is left to the caller via `delete_vm` or
    /// an explicit cleanup step).
    pub async fn send_migration(
        &self,
        vm_id: &str,
        destination_url: &str,
    ) -> Result<(), VmManagerError> {
        info!("Sending migration for VM {} to {}", vm_id, destination_url);

        let socket_path = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
            instance.socket_path.clone()
        };

        let body = format!(r#"{{"destination_url":"{}"}}"#, destination_url);
        Self::send_api_request(
            &socket_path,
            "PUT",
            "/api/v1/vm.send-migration",
            Some(&body),
        )
        .await
        .map_err(|e| VmManagerError::MigrationError(format!("vm.send-migration failed: {}", e)))?;

        // Mark the source instance as Shutdown.  We keep it in the map so
        // the control plane can call delete_vm() to clean up TAP devices and
        // other host resources after confirming migration success.
        {
            let mut vms = self.vms.lock().await;
            if let Some(instance) = vms.get_mut(vm_id) {
                instance.status = VmStatus::Shutdown;
            }
        }

        info!(
            "VM {} migrated out successfully to {}",
            vm_id, destination_url
        );
        Ok(())
    }

    /// Delete a VM
    pub async fn delete_vm(&self, vm_id: &str) -> Result<(), VmManagerError> {
        info!("Deleting VM: {}", vm_id);

        let mut instance = {
            let mut vms = self.vms.lock().await;
            vms.remove(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?
        };

        // Try to shutdown via SDK first
        if let Err(e) = instance.vm.shutdown().await {
            warn!("Failed to shutdown VM via SDK: {}", e);
        }

        // Kill the process if we have it
        if let Some(mut process) = instance.process.take()
            && let Err(e) = process.kill().await
        {
            warn!("Failed to kill CH process: {}", e);
        }

        // Clean up socket
        if instance.socket_path.exists() {
            let _ = tokio::fs::remove_file(&instance.socket_path).await;
        }

        // Clean up persisted config
        let config_path = self.config_path(vm_id);
        if config_path.exists() {
            let _ = tokio::fs::remove_file(&config_path).await;
        }

        // Clean up cloud-init seed image if present
        let seed_path = self.cloud_init_seed_path(vm_id);
        if tokio::fs::try_exists(&seed_path).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&seed_path).await;
        }

        // Clean up TAP devices created by qarax-node
        for tap in &instance.tap_devices {
            Self::delete_tap_device(tap).await;
        }

        // Stop passt backends created by qarax-node
        Self::cleanup_passt_processes(&mut instance.passt_processes).await;

        // Clean up any virtiofsd processes for this VM's fs devices
        if let Some(store) = &self.image_store_manager {
            let fs_count = instance.proto_config.fs.len();
            for i in 0..fs_count {
                store.cleanup_vm(&format!("{}-fs{}", vm_id, i)).await;
            }
        }

        // Unmount OverlayBD device if this VM used one
        if instance.has_overlaybd
            && let Some(obd_manager) = &self.overlaybd_manager
        {
            obd_manager.unmount(vm_id).await;
        }

        info!("VM {} deleted successfully", vm_id);
        Ok(())
    }

    /// Get VM info
    pub async fn get_vm_info(&self, vm_id: &str) -> Result<VmState, VmManagerError> {
        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let mut state = instance.to_vm_state();

        // Try to get live status from CH via SDK
        if let Ok(info) = instance.vm.get_info().await {
            state.status = match info.state {
                models::vm_info::State::Created => VmStatus::Created.into(),
                models::vm_info::State::Running => VmStatus::Running.into(),
                models::vm_info::State::Paused => VmStatus::Paused.into(),
                models::vm_info::State::Shutdown => VmStatus::Shutdown.into(),
            };
            state.memory_actual_size = info.memory_actual_size;
            instance.status = VmStatus::try_from(state.status).unwrap_or(VmStatus::Unknown);
        }

        Ok(state)
    }

    /// Get VM counters from Cloud Hypervisor's /vm.counters endpoint
    pub async fn get_vm_counters(
        &self,
        vm_id: &str,
    ) -> Result<HashMap<String, HashMap<String, i64>>, VmManagerError> {
        let socket_path = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
            instance.socket_path.clone()
        };

        let body =
            match Self::send_api_request(&socket_path, "GET", "/api/v1/vm.counters", None).await {
                Ok(b) => b,
                Err(e) => {
                    debug!("VM {} counters not available: {}", vm_id, e);
                    return Ok(HashMap::new());
                }
            };

        if body.is_empty() {
            return Ok(HashMap::new());
        }

        serde_json::from_str(&body).map_err(|e| VmManagerError::ProcessError(e.to_string()))
    }

    /// List all VMs
    pub async fn list_vms(&self) -> Vec<VmState> {
        let vms = self.vms.lock().await;
        vms.values()
            .map(|instance| instance.to_vm_state())
            .collect()
    }

    /// Add a network device to a VM
    pub async fn add_network_device(
        &self,
        vm_id: &str,
        config: &ProtoNetConfig,
    ) -> Result<(), VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let sdk_config = Self::proto_net_to_sdk(config);
        let body = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        Self::send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.add-net",
            Some(&body),
        )
        .await?;

        Ok(())
    }

    async fn remove_device_by_id(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), VmManagerError> {
        let socket_path = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
            instance.socket_path.clone()
        };

        let body = serde_json::json!({ "id": device_id }).to_string();
        Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.remove-device", Some(&body))
            .await?;
        Ok(())
    }

    /// Remove a network device from a VM
    pub async fn remove_network_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), VmManagerError> {
        self.remove_device_by_id(vm_id, device_id).await
    }

    /// Add a disk device to a VM
    pub async fn add_disk_device(
        &self,
        vm_id: &str,
        config: &ProtoDiskConfig,
    ) -> Result<(), VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let sdk_config = Self::proto_disk_to_sdk(config);
        let body = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        Self::send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.add-disk",
            Some(&body),
        )
        .await?;

        Ok(())
    }

    /// Remove a disk device from a VM
    pub async fn remove_disk_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), VmManagerError> {
        self.remove_device_by_id(vm_id, device_id).await
    }

    /// Send a raw API request to Cloud Hypervisor
    async fn send_api_request(
        socket_path: &PathBuf,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<String, VmManagerError> {
        let stream = UnixStream::connect(socket_path)
            .await
            .map_err(VmManagerError::SpawnError)?;

        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| VmManagerError::ProcessError(e.to_string()))?;

        tokio::spawn(conn);

        let request = if let Some(body_str) = body {
            let body_bytes = Bytes::from(body_str.to_string());
            Request::builder()
                .method(method)
                .uri(format!("http://localhost{}", path))
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .body(BoxBody::new(Full::new(body_bytes)))
                .map_err(|e| VmManagerError::ProcessError(e.to_string()))?
        } else {
            Request::builder()
                .method(method)
                .uri(format!("http://localhost{}", path))
                .header("Accept", "application/json")
                .body(BoxBody::new(Empty::new()))
                .map_err(|e| VmManagerError::ProcessError(e.to_string()))?
        };

        let response = sender
            .send_request(request)
            .await
            .map_err(|e| VmManagerError::ProcessError(e.to_string()))?;

        let status = response.status();

        // Read response body
        let mut body_bytes = http_body_util::BodyStream::new(response.into_body());
        let mut bytes = bytes::BytesMut::new();
        while let Some(chunk) = body_bytes.next().await {
            if let Ok(chunk) = chunk
                && let Ok(data) = chunk.into_data()
            {
                bytes.extend_from_slice(&data);
            }
        }

        let body = String::from_utf8_lossy(&bytes).to_string();

        if !status.is_success() {
            return Err(VmManagerError::ProcessError(format!(
                "API request {} {} failed: HTTP {} — {}",
                method, path, status, body
            )));
        }

        Ok(body)
    }

    /// Convert proto VmConfig to SDK VmConfig
    fn proto_to_sdk_config(&self, config: &ProtoVmConfig) -> Result<VmConfig, VmManagerError> {
        // Build payload config (required)
        let payload = config
            .payload
            .as_ref()
            .map(Self::proto_payload_to_sdk)
            .unwrap_or_else(|| PayloadConfig {
                firmware: None,
                kernel: None,
                cmdline: None,
                initramfs: None,
                igvm: None,
                host_data: None,
            });

        let mut sdk_config = VmConfig::new(payload);

        // CPU config
        if let Some(cpus) = &config.cpus {
            sdk_config.cpus = Some(Box::new(Self::proto_cpus_to_sdk(cpus)));
        }

        // Memory config
        if let Some(memory) = &config.memory {
            sdk_config.memory = Some(Box::new(Self::proto_memory_to_sdk(memory)));
        }

        // Disks
        if !config.disks.is_empty() {
            sdk_config.disks = Some(config.disks.iter().map(Self::proto_disk_to_sdk).collect());
        }

        // Networks
        if !config.networks.is_empty() {
            sdk_config.net = Some(config.networks.iter().map(Self::proto_net_to_sdk).collect());
        }

        // RNG
        if let Some(rng) = &config.rng {
            sdk_config.rng = Some(Box::new(Self::proto_rng_to_sdk(rng)));
        }

        // Serial console
        if let Some(serial) = &config.serial {
            sdk_config.serial = Some(Box::new(Self::proto_console_to_sdk(serial)));
        }

        // Console
        if let Some(console) = &config.console {
            sdk_config.console = Some(Box::new(Self::proto_console_to_sdk(console)));
        }

        // Filesystems (virtiofs)
        if !config.fs.is_empty() {
            sdk_config.fs = Some(config.fs.iter().map(Self::proto_fs_to_sdk).collect());
        }

        // VFIO devices (GPU passthrough)
        if !config.devices.is_empty() {
            sdk_config.devices = Some(
                config
                    .devices
                    .iter()
                    .map(Self::proto_vfio_device_to_sdk)
                    .collect(),
            );
        }

        Ok(sdk_config)
    }

    fn proto_cpus_to_sdk(cpus: &ProtoCpusConfig) -> CpusConfig {
        CpusConfig {
            boot_vcpus: cpus.boot_vcpus,
            max_vcpus: cpus.max_vcpus,
            topology: cpus
                .topology
                .as_ref()
                .map(|t| Box::new(Self::proto_topology_to_sdk(t))),
            kvm_hyperv: cpus.kvm_hyperv,
            max_phys_bits: cpus.max_phys_bits,
            affinity: None,
            features: None,
            nested: None,
        }
    }

    fn proto_topology_to_sdk(topology: &ProtoCpuTopology) -> models::CpuTopology {
        models::CpuTopology {
            threads_per_core: topology.threads_per_core,
            cores_per_die: topology.cores_per_die,
            dies_per_package: topology.dies_per_package,
            packages: topology.packages,
        }
    }

    fn proto_memory_to_sdk(memory: &ProtoMemoryConfig) -> MemoryConfig {
        MemoryConfig {
            size: memory.size,
            hotplug_size: memory.hotplug_size,
            hotplugged_size: None,
            mergeable: memory.mergeable,
            hotplug_method: None,
            shared: memory.shared,
            hugepages: memory.hugepages,
            hugepage_size: memory.hugepage_size,
            prefault: memory.prefault,
            thp: memory.thp,
            zones: None,
        }
    }

    fn proto_payload_to_sdk(payload: &ProtoPayloadConfig) -> PayloadConfig {
        PayloadConfig {
            firmware: payload.firmware.clone(),
            kernel: payload.kernel.clone(),
            cmdline: payload.cmdline.clone(),
            initramfs: payload.initramfs.clone(),
            igvm: None,
            host_data: None,
        }
    }

    fn proto_disk_to_sdk(disk: &ProtoDiskConfig) -> models::DiskConfig {
        models::DiskConfig {
            path: disk.path.clone(),
            readonly: disk.readonly,
            direct: disk.direct,
            iommu: None,
            num_queues: disk.num_queues,
            queue_size: disk.queue_size,
            vhost_user: disk.vhost_user,
            vhost_socket: disk.vhost_socket.clone(),
            rate_limiter_config: disk
                .rate_limiter
                .as_ref()
                .map(|r| Box::new(Self::proto_rate_limiter_to_sdk(r))),
            pci_segment: disk.pci_segment,
            id: Some(disk.id.clone()),
            serial: disk.serial.clone(),
            rate_limit_group: disk.rate_limit_group.clone(),
            queue_affinity: None,
            backing_files: None,
            // Explicitly set raw to prevent CH from autodetecting and disabling
            // sector 0 writes, which breaks ext4 superblock updates.
            image_type: Some(models::DiskImageType::Raw),
            sparse: None,
        }
    }

    fn proto_net_to_sdk(net: &ProtoNetConfig) -> models::NetConfig {
        models::NetConfig {
            tap: net.tap.clone(),
            ip: net.ip.clone(),
            mask: net.mask.clone(),
            mac: net.mac.clone(),
            host_mac: net.host_mac.clone(),
            mtu: net.mtu,
            iommu: net.iommu,
            num_queues: net.num_queues,
            queue_size: net.queue_size,
            vhost_user: net.vhost_user,
            vhost_socket: net.vhost_socket.clone(),
            vhost_mode: net.vhost_mode.map(|m| {
                if m == ProtoVhostMode::Server as i32 {
                    "Server".to_string()
                } else {
                    "Client".to_string()
                }
            }),
            id: Some(net.id.clone()),
            pci_segment: net.pci_segment,
            rate_limiter_config: net
                .rate_limiter
                .as_ref()
                .map(|r| Box::new(Self::proto_rate_limiter_to_sdk(r))),
            offload_tso: net.offload_tso,
            offload_ufo: net.offload_ufo,
            offload_csum: net.offload_csum,
        }
    }

    fn proto_rng_to_sdk(rng: &ProtoRngConfig) -> models::RngConfig {
        models::RngConfig {
            src: rng.src.clone(),
            iommu: rng.iommu,
        }
    }

    fn proto_console_to_sdk(console: &ProtoConsoleConfig) -> models::ConsoleConfig {
        let mode = match ProtoConsoleMode::try_from(console.mode) {
            Ok(ProtoConsoleMode::Off) => ConsoleMode::Off,
            Ok(ProtoConsoleMode::Pty) => ConsoleMode::Pty,
            Ok(ProtoConsoleMode::Tty) => ConsoleMode::Tty,
            Ok(ProtoConsoleMode::File) => ConsoleMode::File,
            Ok(ProtoConsoleMode::Socket) => ConsoleMode::Socket,
            Ok(ProtoConsoleMode::Null) => ConsoleMode::Null,
            _ => ConsoleMode::Null,
        };

        models::ConsoleConfig {
            file: console.file.clone(),
            socket: console.socket.clone(),
            mode,
            iommu: console.iommu,
        }
    }

    fn proto_fs_to_sdk(fs: &ProtoFsConfig) -> FsConfig {
        FsConfig {
            tag: fs.tag.clone(),
            socket: fs.socket.clone(),
            num_queues: fs.num_queues,
            queue_size: fs.queue_size,
            pci_segment: fs.pci_segment,
            id: fs.id.clone(),
        }
    }

    fn proto_vfio_device_to_sdk(device: &ProtoVfioDeviceConfig) -> models::DeviceConfig {
        models::DeviceConfig {
            path: device.path.clone(),
            iommu: device.iommu,
            pci_segment: device.pci_segment,
            id: Some(device.id.clone()),
            x_nv_gpudirect_clique: None,
        }
    }

    /// Add a VFIO device (e.g., GPU) to a running VM
    pub async fn add_device(
        &self,
        vm_id: &str,
        config: &ProtoVfioDeviceConfig,
    ) -> Result<(), VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let sdk_config = Self::proto_vfio_device_to_sdk(config);
        let body = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        Self::send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.add-device",
            Some(&body),
        )
        .await?;

        Ok(())
    }

    /// Remove a VFIO device from a running VM
    pub async fn remove_device(&self, vm_id: &str, device_id: &str) -> Result<(), VmManagerError> {
        self.remove_device_by_id(vm_id, device_id).await
    }

    /// Add a filesystem (virtiofs) device to a running VM
    pub async fn add_fs_device(
        &self,
        vm_id: &str,
        config: &ProtoFsConfig,
    ) -> Result<(), VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let sdk_config = Self::proto_fs_to_sdk(config);
        let body = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        Self::send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.add-fs",
            Some(&body),
        )
        .await?;

        Ok(())
    }

    /// Remove a filesystem device from a running VM
    pub async fn remove_fs_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), VmManagerError> {
        self.remove_device_by_id(vm_id, device_id).await
    }

    fn proto_rate_limiter_to_sdk(
        rate_limiter: &ProtoRateLimiterConfig,
    ) -> models::RateLimiterConfig {
        models::RateLimiterConfig {
            bandwidth: rate_limiter
                .bandwidth
                .as_ref()
                .map(|b| Box::new(Self::proto_token_bucket_to_sdk(b))),
            ops: rate_limiter
                .ops
                .as_ref()
                .map(|o| Box::new(Self::proto_token_bucket_to_sdk(o))),
        }
    }

    fn proto_token_bucket_to_sdk(bucket: &ProtoTokenBucket) -> models::TokenBucket {
        models::TokenBucket {
            size: bucket.size,
            refill_time: bucket.refill_time,
            one_time_burst: bucket.one_time_burst,
        }
    }

    /// Get the PTY path for a VM's serial or console device.
    /// This queries Cloud Hypervisor's API to retrieve PTY device paths.
    /// Returns (serial_pty_path, console_pty_path) if available.
    pub async fn get_pty_paths(
        &self,
        vm_id: &str,
    ) -> Result<(Option<String>, Option<String>), VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        // Query Cloud Hypervisor for PTY info via vm.info
        // The v1 API doesn't expose PTY paths directly via HTTP, so we need to check
        // if the PTY paths are in the filesystem based on CH's behavior.
        // CH creates PTY devices and links them predictably.

        let serial_pty = if instance
            .proto_config
            .serial
            .as_ref()
            .map(|s| s.mode == ProtoConsoleMode::Pty as i32)
            .unwrap_or(false)
        {
            // Cloud Hypervisor uses /dev/pts/X for PTY devices
            // We need to query the actual path via the /proc filesystem or API
            // For now, we'll track this in the instance after creation
            instance.serial_pty_path.clone()
        } else {
            None
        };

        let console_pty = if instance
            .proto_config
            .console
            .as_ref()
            .map(|c| c.mode == ProtoConsoleMode::Pty as i32)
            .unwrap_or(false)
        {
            instance.console_pty_path.clone()
        } else {
            None
        };

        Ok((serial_pty, console_pty))
    }

    /// Query Cloud Hypervisor's vm.info API to obtain PTY device paths.
    ///
    /// When a serial or console device is configured in PTY mode, CH allocates
    /// a PTY and exposes the slave device path in the vm.info response under
    /// `config.serial.file` / `config.console.file`. This is more reliable than
    /// log parsing because CH doesn't necessarily log the PTY path at all log levels.
    async fn query_pty_paths(
        &self,
        socket_path: &PathBuf,
        config: &ProtoVmConfig,
    ) -> (Option<String>, Option<String>) {
        let serial_is_pty = config
            .serial
            .as_ref()
            .map(|s| s.mode == ProtoConsoleMode::Pty as i32)
            .unwrap_or(false);
        let console_is_pty = config
            .console
            .as_ref()
            .map(|c| c.mode == ProtoConsoleMode::Pty as i32)
            .unwrap_or(false);

        if !serial_is_pty && !console_is_pty {
            return (None, None);
        }

        let body = match Self::send_api_request(socket_path, "GET", "/api/v1/vm.info", None).await {
            Ok(b) => b,
            Err(e) => {
                debug!("Failed to query vm.info for PTY paths: {}", e);
                return (None, None);
            }
        };

        let info: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to parse vm.info response: {}", e);
                return (None, None);
            }
        };

        let serial_pty = if serial_is_pty {
            info["config"]["serial"]["file"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| {
                    info!("Discovered serial PTY path via vm.info: {}", s);
                    s.to_string()
                })
        } else {
            None
        };

        let console_pty = if console_is_pty {
            info["config"]["console"]["file"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| {
                    info!("Discovered console PTY path via vm.info: {}", s);
                    s.to_string()
                })
        } else {
            None
        };

        (serial_pty, console_pty)
    }

    /// Get the serial console PTY path if available.
    ///
    /// If the path was not discovered at create/start time (e.g. for recovered
    /// VMs), queries Cloud Hypervisor's vm.info API to obtain it on demand and
    /// caches the result in the instance for subsequent calls.
    pub async fn get_serial_pty_path(&self, vm_id: &str) -> Result<Option<String>, VmManagerError> {
        // Fast path: return cached value if available.
        let (socket_path, proto_config) = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

            if let Some(path) = &instance.serial_pty_path {
                return Ok(Some(path.clone()));
            }

            (instance.socket_path.clone(), instance.proto_config.clone())
        };

        let (pty_path, _) = self.query_pty_paths(&socket_path, &proto_config).await;

        // Cache the result so subsequent calls don't re-query the API
        if let Some(path) = &pty_path {
            info!("Discovered serial PTY path via vm.info: {}", path);
            let mut vms = self.vms.lock().await;
            if let Some(instance) = vms.get_mut(vm_id) {
                instance.serial_pty_path = Some(path.clone());
                if !instance
                    .proto_config
                    .serial
                    .as_ref()
                    .map(|serial| serial.mode == ProtoConsoleMode::Pty as i32)
                    .unwrap_or(false)
                {
                    instance.proto_config.serial = Some(ProtoConsoleConfig {
                        mode: ProtoConsoleMode::Pty as i32,
                        file: None,
                        socket: None,
                        iommu: None,
                    });
                }
            }
        }

        Ok(pty_path)
    }

    /// Check whether the Cloud Hypervisor process for a VM is still alive.
    ///
    /// Returns `false` if the process has exited, is a zombie, or was never
    /// tracked (e.g. a recovered VM).
    pub async fn is_vm_process_alive(&self, vm_id: &str) -> bool {
        let mut vms = self.vms.lock().await;
        let Some(instance) = vms.get_mut(vm_id) else {
            return false;
        };
        match &mut instance.process {
            Some(child) => child.try_wait().ok().flatten().is_none(),
            None => {
                // No process handle (recovered VM) — check socket reachability
                instance.socket_path.exists()
            }
        }
    }

    /// Get the console device PTY path if available
    pub async fn get_console_pty_path(
        &self,
        vm_id: &str,
    ) -> Result<Option<String>, VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        // Check if console is configured in PTY mode
        if let Some(console) = &instance.proto_config.console
            && console.mode == ProtoConsoleMode::Pty as i32
        {
            return Ok(instance.console_pty_path.clone());
        }

        Ok(None)
    }
}

impl Drop for VmManager {
    fn drop(&mut self) {
        info!("VmManager dropped, all VMs will be terminated");
    }
}
