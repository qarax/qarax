//! VMM abstraction layer.
//!
//! `VmmManager` is the trait that all hypervisor backends implement.  The VM
//! service dispatches lifecycle operations to the correct backend based on the
//! `HypervisorType` field in the VM config.

use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;

use crate::rpc::node::{
    DiskConfig as ProtoDiskConfig, ExecVmResponse, ImageFormat, NetConfig as ProtoNetConfig,
    SendMigrationRequest, VfioDeviceConfig as ProtoVfioDeviceConfig, VmConfig as ProtoVmConfig,
    VmState,
};

#[derive(Debug, thiserror::Error)]
pub enum VmmError {
    #[error("VM {0} not found")]
    VmNotFound(String),

    #[error("VM {0} already exists")]
    VmAlreadyExists(String),

    #[error("Failed to spawn VMM process: {0}")]
    SpawnError(std::io::Error),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("TAP device error: {0}")]
    TapError(String),

    #[error("OverlayBD error: {0}")]
    OverlayBdError(String),

    #[error("Migration error: {0}")]
    MigrationError(String),

    #[error("Storage backend error: {0}")]
    StorageError(String),

    #[error("Exec agent not configured for VM {0}")]
    ExecUnavailable(String),

    #[error("Exec request is invalid: {0}")]
    ExecInvalid(String),

    #[error("Exec guest agent error: {0}")]
    ExecError(String),

    #[error("Exec guest agent timed out after {0}s")]
    ExecTimeout(u64),

    #[error("Operation not supported by this hypervisor: {0}")]
    Unsupported(&'static str),
}

impl From<crate::cloud_hypervisor::VmManagerError> for VmmError {
    fn from(e: crate::cloud_hypervisor::VmManagerError) -> Self {
        use crate::cloud_hypervisor::VmManagerError as E;
        match e {
            E::VmNotFound(id) => VmmError::VmNotFound(id),
            E::VmAlreadyExists(id) => VmmError::VmAlreadyExists(id),
            E::SpawnError(e) => VmmError::SpawnError(e),
            E::InvalidConfig(msg) => VmmError::InvalidConfig(msg),
            E::ProcessError(msg) => VmmError::ProcessError(msg),
            E::TapError(msg) => VmmError::TapError(msg),
            E::OverlayBdError(e) => VmmError::OverlayBdError(e.to_string()),
            E::MigrationError(msg) => VmmError::MigrationError(msg),
            E::StorageError(msg) => VmmError::StorageError(msg),
            E::ExecUnavailable(msg) => VmmError::ExecUnavailable(msg),
            E::ExecInvalid(msg) => VmmError::ExecInvalid(msg),
            E::ExecError(msg) => VmmError::ExecError(msg),
            E::ExecTimeout(secs) => VmmError::ExecTimeout(secs),
            E::SdkError(e) => VmmError::ProcessError(format!("SDK error: {}", e)),
        }
    }
}

/// Core VM lifecycle operations that all hypervisor backends must implement.
///
/// Infrastructure concerns (storage backends, OverlayBD, networking, node info)
/// stay in `VmServiceImpl` since they are either VMM-agnostic or CH-specific.
#[async_trait]
pub trait VmmManager: Send + Sync + 'static {
    // ── Core lifecycle ──────────────────────────────────────────────────────

    async fn create_vm(&self, config: ProtoVmConfig) -> Result<VmState, VmmError>;
    async fn start_vm(&self, vm_id: &str) -> Result<(), VmmError>;
    async fn stop_vm(&self, vm_id: &str) -> Result<(), VmmError>;
    async fn force_stop_vm(&self, vm_id: &str) -> Result<(), VmmError>;
    async fn pause_vm(&self, vm_id: &str) -> Result<(), VmmError>;
    async fn resume_vm(&self, vm_id: &str) -> Result<(), VmmError>;
    async fn delete_vm(&self, vm_id: &str) -> Result<(), VmmError>;

    async fn get_vm_info(&self, vm_id: &str) -> Result<VmState, VmmError>;
    async fn list_vms(&self) -> Vec<VmState>;

    // ── Snapshots ───────────────────────────────────────────────────────────

    async fn snapshot_vm(&self, vm_id: &str, destination_url: &str) -> Result<(), VmmError>;
    async fn restore_vm(&self, vm_id: &str, source_url: &str) -> Result<(), VmmError>;

    // ── Recovery ────────────────────────────────────────────────────────────

    /// Scan for surviving VMM processes and reconnect to them on node restart.
    async fn recover_vms(&self);

    // ── Runtime info ────────────────────────────────────────────────────────

    fn runtime_dir(&self) -> &Path;

    // ── Operations with default Unsupported responses ───────────────────────
    // Cloud Hypervisor supports all of these; Firecracker returns Unsupported.

    async fn add_network_device(
        &self,
        _vm_id: &str,
        _config: &ProtoNetConfig,
    ) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("add_network_device"))
    }

    async fn remove_network_device(&self, _vm_id: &str, _device_id: &str) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("remove_network_device"))
    }

    async fn add_disk_device(
        &self,
        _vm_id: &str,
        _config: &ProtoDiskConfig,
    ) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("add_disk_device"))
    }

    async fn remove_disk_device(&self, _vm_id: &str, _device_id: &str) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("remove_disk_device"))
    }

    async fn add_device(
        &self,
        _vm_id: &str,
        _config: &ProtoVfioDeviceConfig,
    ) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("add_device"))
    }

    async fn remove_device(&self, _vm_id: &str, _device_id: &str) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("remove_device"))
    }

    async fn resize_vm(
        &self,
        _vm_id: &str,
        _desired_vcpus: Option<i32>,
        _desired_ram: Option<i64>,
    ) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("resize_vm"))
    }

    async fn resize_disk(
        &self,
        _vm_id: &str,
        _disk_id: &str,
        _path: &str,
        _new_size: i64,
        _image_format: ImageFormat,
    ) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("resize_disk"))
    }

    async fn receive_migration(
        &self,
        _vm_id: &str,
        _config: ProtoVmConfig,
        _port: u16,
    ) -> Result<String, VmmError> {
        Err(VmmError::Unsupported("receive_migration"))
    }

    async fn send_migration(&self, _req: &SendMigrationRequest) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("send_migration"))
    }

    async fn exec_vm(
        &self,
        _vm_id: &str,
        _command: Vec<String>,
        _timeout_secs: Option<u64>,
    ) -> Result<ExecVmResponse, VmmError> {
        Err(VmmError::Unsupported("exec_vm"))
    }

    async fn get_vm_counters(
        &self,
        _vm_id: &str,
    ) -> Result<HashMap<String, HashMap<String, i64>>, VmmError> {
        Err(VmmError::Unsupported("get_vm_counters"))
    }

    async fn get_serial_pty_path(&self, _vm_id: &str) -> Result<Option<String>, VmmError> {
        Err(VmmError::Unsupported("get_serial_pty_path"))
    }

    async fn is_vm_process_alive(&self, _vm_id: &str) -> bool {
        false
    }
}
