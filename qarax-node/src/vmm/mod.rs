//! VMM abstraction layer.
//!
//! `VmmManager` is the trait that all hypervisor backends implement.  The VM
//! service dispatches lifecycle operations to the correct backend based on the
//! `HypervisorType` field in the VM config.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;

use crate::rpc::node::{
    DiskConfig as ProtoDiskConfig, ExecVmResponse, Fork, ImageFormat, NetConfig as ProtoNetConfig,
    SendMigrationRequest, VfioDeviceConfig as ProtoVfioDeviceConfig, VmConfig as ProtoVmConfig,
    VmState,
};

pub mod fork;
pub mod guest_agent;
pub mod memory;
pub(crate) mod vms;

/// How long a VMM gets to answer a request it serves at once (its VM's
/// status, a shutdown) before it is taken to have stopped answering: far
/// longer than one takes on a loaded host. One busy with a long request (a
/// snapshot, an incoming migration) can take longer still, and the callers
/// carry on without the answer: a status query keeps the VM's last known
/// status. The control plane's VM monitor asks for a host's VMs in turn, so
/// each VMM there that has stopped answering adds this much to its tick.
pub const VMM_ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

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

impl From<crate::cloud_init::CloudInitError> for VmmError {
    fn from(e: crate::cloud_init::CloudInitError) -> Self {
        use crate::cloud_init::CloudInitError as E;
        match e {
            E::Io(e) => VmmError::SpawnError(e),
            E::Fat(msg) => VmmError::InvalidConfig(msg),
        }
    }
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

    /// The socket the VM's hypervisor serves its API on.
    fn api_socket(&self, vm_id: &str) -> PathBuf;

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

    /// The host end of a running VM's guest-agent vsock.
    async fn guest_agent_socket(&self, vm_id: &str) -> Result<PathBuf, VmmError>;

    /// Load `forks`, new VMs that continue from `source_id`'s current state,
    /// each with its own NICs, and leave them paused, for their firewall to
    /// come first. The source keeps running. Either all of them load or none
    /// is left.
    async fn fork_vm(&self, _source_id: &str, _forks: &[Fork]) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("fork_vm"))
    }

    /// Resume forks of `source_id` that [`VmmManager::fork_vm`] loaded, and
    /// tell each one's guest that it is a fork, and its NICs' addresses.
    async fn resume_forks(&self, _source_id: &str, _fork_ids: &[String]) -> Result<(), VmmError> {
        Err(VmmError::Unsupported("resume_forks"))
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

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;
    use tokio::sync::{Notify, watch};

    /// Longer than a request to a VMM that doesn't answer takes to be given
    /// up, even on a loaded machine.
    pub(crate) const PAST_VMM_ANSWER_TIMEOUT: Duration =
        Duration::from_secs(super::VMM_ANSWER_TIMEOUT.as_secs() + 5);

    /// Serves `listener` as a VMM that answers each request with the JSON
    /// `answer` holds, and while it holds none, as one that has stopped
    /// answering: it takes requests and answers none, and notifies the
    /// returned `Notify` of each.
    pub(crate) fn serve_vmm(
        listener: UnixListener,
        answer: watch::Receiver<Option<String>>,
    ) -> Arc<Notify> {
        let asked = Arc::new(Notify::new());
        let notify_asked = asked.clone();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (asked, answer) = (notify_asked.clone(), answer.clone());
                tokio::spawn(async move {
                    let mut request = vec![0; 4096];
                    // One connection can carry several requests (an SDK handle's does).
                    while let Ok(1..) = stream.read(&mut request).await {
                        let body = answer.borrow().clone();
                        let Some(body) = body else {
                            asked.notify_one();
                            return std::future::pending().await;
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream.write_all(response.as_bytes()).await.unwrap();
                    }
                });
            }
        });
        asked
    }
}
