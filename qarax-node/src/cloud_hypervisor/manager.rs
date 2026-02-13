//! VM Manager for Cloud Hypervisor processes
//!
//! Manages the lifecycle of Cloud Hypervisor processes using the SDK.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

use bytes::Bytes;
use cloud_hypervisor_sdk::client::TokioIo;
use cloud_hypervisor_sdk::machine::{Machine, MachineConfig, VM};
use cloud_hypervisor_sdk::models::{
    self, CpusConfig, MemoryConfig, PayloadConfig, VmConfig, console_config::Mode as ConsoleMode,
};
use futures::stream::StreamExt;
use http_body_util::{Empty, Full, combinators::BoxBody};
use hyper::Request;
use tokio::net::UnixStream;

use crate::rpc::node::{
    ConsoleConfig as ProtoConsoleConfig, ConsoleMode as ProtoConsoleMode,
    CpuTopology as ProtoCpuTopology, CpusConfig as ProtoCpusConfig, DiskConfig as ProtoDiskConfig,
    MemoryConfig as ProtoMemoryConfig, NetConfig as ProtoNetConfig,
    PayloadConfig as ProtoPayloadConfig, RateLimiterConfig as ProtoRateLimiterConfig,
    RngConfig as ProtoRngConfig, TokenBucket as ProtoTokenBucket, VhostMode as ProtoVhostMode,
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
}

/// Represents a running VM instance
struct VmInstance {
    /// The VM configuration (proto format)
    proto_config: ProtoVmConfig,
    /// Cloud Hypervisor process
    process: Child,
    /// SDK VM handle
    vm: VM<'static>,
    /// Path to the API socket
    socket_path: PathBuf,
    /// Current status
    status: VmStatus,
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
}

impl VmManager {
    /// Create a new VM manager
    pub fn new(runtime_dir: impl Into<PathBuf>, ch_binary: impl Into<PathBuf>) -> Self {
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
        }
    }

    /// Get the socket path for a VM
    fn socket_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.sock", vm_id))
    }

    /// Get the log path for a VM
    fn log_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.log", vm_id))
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

        // Ensure runtime directory exists
        tokio::fs::create_dir_all(&self.runtime_dir)
            .await
            .map_err(VmManagerError::SpawnError)?;

        let socket_path = self.socket_path(&vm_id);
        let log_path = self.log_path(&vm_id);

        // Remove old socket if it exists
        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }

        // Spawn Cloud Hypervisor process directly
        debug!(
            "Spawning Cloud Hypervisor with socket: {}",
            socket_path.display()
        );

        let log_file = std::fs::File::create(&log_path).map_err(VmManagerError::SpawnError)?;

        let process = Command::new(&self.ch_binary)
            .arg("--api-socket")
            .arg(&socket_path)
            .stdout(log_file.try_clone().map_err(VmManagerError::SpawnError)?)
            .stderr(log_file)
            .kill_on_drop(true)
            .spawn()
            .map_err(VmManagerError::SpawnError)?;

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
                Err(e) => return Err(VmManagerError::SpawnError(e)),
            }
        }

        // Convert proto config to SDK config
        let sdk_config = self.proto_to_sdk_config(&config)?;
        let json_config = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        debug!("Creating VM with config: {}", json_config);

        // Send create request via raw API
        self.send_api_request(&socket_path, "PUT", "/api/v1/vm.create", Some(&json_config))
            .await?;

        info!("VM {} created on Cloud Hypervisor", vm_id);

        // Parse VM ID as UUID for SDK
        let vm_uuid = Uuid::parse_str(&vm_id)
            .map_err(|e| VmManagerError::InvalidConfig(format!("Invalid VM ID: {}", e)))?;

        // Connect to the CH instance via SDK
        let socket_path_static: &'static PathBuf = Box::leak(Box::new(socket_path.clone()));
        let ch_binary_static: &'static PathBuf = Box::leak(Box::new(self.ch_binary.clone()));

        let machine_config = MachineConfig {
            vm_id: vm_uuid,
            socket_path: Cow::Borrowed(socket_path_static.as_path()),
            exec_path: Cow::Borrowed(ch_binary_static.as_path()),
        };

        let vm = Machine::connect(machine_config).await?;

        let instance = VmInstance {
            proto_config: config.clone(),
            process,
            vm,
            socket_path,
            status: VmStatus::Created,
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

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        instance.vm.boot().await?;
        instance.status = VmStatus::Running;

        info!("VM {} started successfully", vm_id);
        Ok(())
    }

    /// Stop a VM
    pub async fn stop_vm(&self, vm_id: &str) -> Result<(), VmManagerError> {
        info!("Stopping VM: {}", vm_id);

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        instance.vm.shutdown().await?;
        instance.status = VmStatus::Shutdown;

        info!("VM {} stopped successfully", vm_id);
        Ok(())
    }

    /// Pause a VM
    pub async fn pause_vm(&self, vm_id: &str) -> Result<(), VmManagerError> {
        info!("Pausing VM: {}", vm_id);

        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        self.send_api_request(&instance.socket_path, "PUT", "/api/v1/vm.pause", None)
            .await?;

        drop(vms);

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

        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        self.send_api_request(&instance.socket_path, "PUT", "/api/v1/vm.resume", None)
            .await?;

        drop(vms);

        let mut vms = self.vms.lock().await;
        if let Some(instance) = vms.get_mut(vm_id) {
            instance.status = VmStatus::Running;
        }

        info!("VM {} resumed successfully", vm_id);
        Ok(())
    }

    /// Delete a VM
    pub async fn delete_vm(&self, vm_id: &str) -> Result<(), VmManagerError> {
        info!("Deleting VM: {}", vm_id);

        let mut vms = self.vms.lock().await;
        let mut instance = vms
            .remove(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        // Try to shutdown via SDK first
        if let Err(e) = instance.vm.shutdown().await {
            warn!("Failed to shutdown VM via SDK: {}", e);
        }

        // Kill the process
        if let Err(e) = instance.process.kill().await {
            warn!("Failed to kill CH process: {}", e);
        }

        // Clean up socket
        if instance.socket_path.exists() {
            let _ = tokio::fs::remove_file(&instance.socket_path).await;
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

        let sdk_config = self.proto_net_to_sdk(config);
        let body = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        self.send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.add-net",
            Some(&body),
        )
        .await?;

        Ok(())
    }

    /// Remove a network device from a VM
    pub async fn remove_network_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let body = serde_json::json!({ "id": device_id }).to_string();
        self.send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.remove-device",
            Some(&body),
        )
        .await?;

        Ok(())
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

        let sdk_config = self.proto_disk_to_sdk(config);
        let body = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        self.send_api_request(
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
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let body = serde_json::json!({ "id": device_id }).to_string();
        self.send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.remove-device",
            Some(&body),
        )
        .await?;

        Ok(())
    }

    /// Send a raw API request to Cloud Hypervisor
    async fn send_api_request(
        &self,
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

        if !response.status().is_success() {
            return Err(VmManagerError::ProcessError(format!(
                "API request failed: HTTP {}",
                response.status()
            )));
        }

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

        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    /// Convert proto VmConfig to SDK VmConfig
    fn proto_to_sdk_config(&self, config: &ProtoVmConfig) -> Result<VmConfig, VmManagerError> {
        // Build payload config (required)
        let payload = config
            .payload
            .as_ref()
            .map(|p| self.proto_payload_to_sdk(p))
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
            sdk_config.cpus = Some(Box::new(self.proto_cpus_to_sdk(cpus)));
        }

        // Memory config
        if let Some(memory) = &config.memory {
            sdk_config.memory = Some(Box::new(self.proto_memory_to_sdk(memory)));
        }

        // Disks
        if !config.disks.is_empty() {
            sdk_config.disks = Some(
                config
                    .disks
                    .iter()
                    .map(|d| self.proto_disk_to_sdk(d))
                    .collect(),
            );
        }

        // Networks
        if !config.networks.is_empty() {
            sdk_config.net = Some(
                config
                    .networks
                    .iter()
                    .map(|n| self.proto_net_to_sdk(n))
                    .collect(),
            );
        }

        // RNG
        if let Some(rng) = &config.rng {
            sdk_config.rng = Some(Box::new(self.proto_rng_to_sdk(rng)));
        }

        // Serial console
        if let Some(serial) = &config.serial {
            sdk_config.serial = Some(Box::new(self.proto_console_to_sdk(serial)));
        }

        // Console
        if let Some(console) = &config.console {
            sdk_config.console = Some(Box::new(self.proto_console_to_sdk(console)));
        }

        Ok(sdk_config)
    }

    fn proto_cpus_to_sdk(&self, cpus: &ProtoCpusConfig) -> CpusConfig {
        CpusConfig {
            boot_vcpus: cpus.boot_vcpus,
            max_vcpus: cpus.max_vcpus,
            topology: cpus
                .topology
                .as_ref()
                .map(|t| Box::new(self.proto_topology_to_sdk(t))),
            kvm_hyperv: cpus.kvm_hyperv,
            max_phys_bits: cpus.max_phys_bits,
            affinity: None,
            features: None,
        }
    }

    fn proto_topology_to_sdk(&self, topology: &ProtoCpuTopology) -> models::CpuTopology {
        models::CpuTopology {
            threads_per_core: topology.threads_per_core,
            cores_per_die: topology.cores_per_die,
            dies_per_package: topology.dies_per_package,
            packages: topology.packages,
        }
    }

    fn proto_memory_to_sdk(&self, memory: &ProtoMemoryConfig) -> MemoryConfig {
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

    fn proto_payload_to_sdk(&self, payload: &ProtoPayloadConfig) -> PayloadConfig {
        PayloadConfig {
            firmware: payload.firmware.clone(),
            kernel: payload.kernel.clone(),
            cmdline: payload.cmdline.clone(),
            initramfs: payload.initramfs.clone(),
            igvm: None,
            host_data: None,
        }
    }

    fn proto_disk_to_sdk(&self, disk: &ProtoDiskConfig) -> models::DiskConfig {
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
                .map(|r| Box::new(self.proto_rate_limiter_to_sdk(r))),
            pci_segment: disk.pci_segment,
            id: Some(disk.id.clone()),
            serial: disk.serial.clone(),
            rate_limit_group: disk.rate_limit_group.clone(),
            queue_affinity: None,
        }
    }

    fn proto_net_to_sdk(&self, net: &ProtoNetConfig) -> models::NetConfig {
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
                .map(|r| Box::new(self.proto_rate_limiter_to_sdk(r))),
            offload_tso: net.offload_tso,
            offload_ufo: net.offload_ufo,
            offload_csum: net.offload_csum,
        }
    }

    fn proto_rng_to_sdk(&self, rng: &ProtoRngConfig) -> models::RngConfig {
        models::RngConfig {
            src: rng.src.clone(),
            iommu: rng.iommu,
        }
    }

    fn proto_console_to_sdk(&self, console: &ProtoConsoleConfig) -> models::ConsoleConfig {
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

    fn proto_rate_limiter_to_sdk(
        &self,
        rate_limiter: &ProtoRateLimiterConfig,
    ) -> models::RateLimiterConfig {
        models::RateLimiterConfig {
            bandwidth: rate_limiter
                .bandwidth
                .as_ref()
                .map(|b| Box::new(self.proto_token_bucket_to_sdk(b))),
            ops: rate_limiter
                .ops
                .as_ref()
                .map(|o| Box::new(self.proto_token_bucket_to_sdk(o))),
        }
    }

    fn proto_token_bucket_to_sdk(&self, bucket: &ProtoTokenBucket) -> models::TokenBucket {
        models::TokenBucket {
            size: bucket.size,
            refill_time: bucket.refill_time,
            one_time_burst: bucket.one_time_burst,
        }
    }
}

impl Drop for VmManager {
    fn drop(&mut self) {
        info!("VmManager dropped, all VMs will be terminated");
    }
}
