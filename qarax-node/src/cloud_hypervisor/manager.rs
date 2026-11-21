//! VM Manager for Cloud Hypervisor processes
//!
//! Manages the lifecycle of Cloud Hypervisor processes using the SDK.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, OwnedMappedMutexGuard, OwnedMutexGuard, watch};
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use bytes::Bytes;
use cloud_hypervisor_sdk::client::TokioIo;
use cloud_hypervisor_sdk::machine::{Machine, MachineConfig, VM};
use cloud_hypervisor_sdk::models::{
    self, ConsoleMode, CpuAffinity, CpusConfig, ImageType, IommuType, MemoryConfig,
    MemoryZoneConfig, NumaConfig, PayloadConfig, VmConfig, VsockConfig as SdkVsockConfig,
};
use futures::stream::StreamExt;
use http_body_util::{Empty, Full, combinators::BoxBody};
use hyper::Request;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use prost::Message;

use crate::overlaybd::OverlayBdManager;
use crate::rpc::node::StoragePoolKind;
use crate::rpc::node::{
    ConsoleConfig as ProtoConsoleConfig, ConsoleMode as ProtoConsoleMode,
    CpuTopology as ProtoCpuTopology, CpusConfig as ProtoCpusConfig, DiskConfig as ProtoDiskConfig,
    ExecVmResponse, ImageFormat as ProtoImageFormat, MemoryConfig as ProtoMemoryConfig,
    MigrationMemoryMode as ProtoMigrationMemoryMode,
    MigrationTimeoutAction as ProtoMigrationTimeoutAction, NetConfig as ProtoNetConfig,
    NumaPlacement as ProtoNumaPlacement, PayloadConfig as ProtoPayloadConfig,
    RateLimiterConfig as ProtoRateLimiterConfig, RngConfig as ProtoRngConfig,
    SendMigrationRequest as ProtoSendMigrationRequest, TokenBucket as ProtoTokenBucket,
    VfioDeviceConfig as ProtoVfioDeviceConfig, VhostMode as ProtoVhostMode,
    VmConfig as ProtoVmConfig, VmState, VmStatus, VsockConfig as ProtoVsockConfig,
};
use crate::storage::StorageBackendRegistry;
use crate::vmm::VMM_ANSWER_TIMEOUT;

/// A directory with the certificates Cloud Hypervisor uses for migration
/// TLS: the migration CA, a server certificate valid for the address this
/// node is registered under (for migrations in), and a client certificate
/// (for migrations out).
#[derive(Debug, Clone)]
pub struct MigrationTlsDir(PathBuf);

impl MigrationTlsDir {
    const FILES: [&str; 5] = [
        "ca-cert.pem",
        "server-cert.pem",
        "server-key.pem",
        "client-cert.pem",
        "client-key.pem",
    ];

    pub fn new(dir: PathBuf) -> Result<Self, String> {
        let missing: Vec<&str> = Self::FILES
            .into_iter()
            .filter(|file| !dir.join(file).is_file())
            .collect();
        if missing.is_empty() {
            Ok(Self(dir))
        } else {
            Err(format!(
                "migration TLS directory {} has no {}",
                dir.display(),
                missing.join(", ")
            ))
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

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
}

/// A VM's config and status, as reads of the node's VMs see them.
struct VmSummary {
    config: ProtoVmConfig,
    status: VmStatus,
}

impl VmSummary {
    fn to_vm_state(&self) -> VmState {
        VmState {
            config: Some(self.config.clone()),
            status: self.status.into(),
            memory_actual_size: None,
        }
    }
}

/// Represents a running VM instance
struct VmInstance {
    /// The VM's config and current status, published for the reads that
    /// don't wait for the VM's lock (see [`ManagedVm`]). Only the instance
    /// sends on it, so only a task holding the lock changes them.
    summary: watch::Sender<VmSummary>,
    /// Cloud Hypervisor process (None for recovered VMs)
    process: Option<Child>,
    /// SDK VM handle
    vm: VM<'static>,
    /// Path to the API socket
    socket_path: PathBuf,
    /// TAP devices created by qarax-node for this VM (cleaned up on delete)
    tap_devices: Vec<String>,
    /// passt backend processes started by qarax-node for this VM
    passt_processes: Vec<Child>,
    /// PTY path for serial console (if PTY mode is enabled)
    serial_pty_path: Option<String>,
    /// PTY path for console device (if PTY mode is enabled)
    console_pty_path: Option<String>,
    /// Host-side UNIX socket used for virtio-vsock guest-agent access
    vsock_socket_path: Option<PathBuf>,
    /// Which storage backend mapped a disk for this VM (if any).
    /// Used by delete_vm to call the correct backend's unmap().
    storage_backend_kind: Option<StoragePoolKind>,
}

impl VmInstance {
    fn config(&self) -> ProtoVmConfig {
        self.summary.borrow().config.clone()
    }

    fn set_config(&self, config: ProtoVmConfig) {
        self.summary.send_modify(|summary| summary.config = config);
    }

    fn status(&self) -> VmStatus {
        self.summary.borrow().status
    }

    fn set_status(&self, status: VmStatus) {
        self.summary.send_modify(|summary| summary.status = status);
    }
}

/// A VM the manager knows. Operations on it hold its lock to use or change its
/// instance, some of them across requests to its VMM (`get_vm_info`, for at
/// most [`VMM_ANSWER_TIMEOUT`]; adding a device; a fork's snapshot of its
/// source), so a VMM that is slow or hung holds up its own VM's operations
/// only. Reads of all the node's VMs take each one's last known config and
/// status from `summary`, without the lock.
struct ManagedVm {
    summary: watch::Receiver<VmSummary>,
    /// `None` once the VM is deleted, for the operations that were waiting.
    instance: Arc<Mutex<Option<VmInstance>>>,
}

impl ManagedVm {
    fn new(instance: VmInstance) -> Self {
        Self {
            summary: instance.summary.subscribe(),
            instance: Arc::new(Mutex::new(Some(instance))),
        }
    }
}

/// A VM's instance, while its lock is held.
type VmGuard = OwnedMappedMutexGuard<Option<VmInstance>, VmInstance>;

/// Manager for Cloud Hypervisor VM instances
pub struct VmManager {
    /// Base directory for VM runtime files (sockets, logs, etc.)
    runtime_dir: PathBuf,
    /// Path to cloud-hypervisor binary
    ch_binary: PathBuf,
    /// The VMs. Locked only to find, add, remove or list them, never across an
    /// await: a std Mutex, which the Send futures here can't hold across one.
    vms: std::sync::Mutex<HashMap<String, ManagedVm>>,
    /// Storage backend registry for attach/detach/map/unmap operations
    storage_backends: StorageBackendRegistry,
    /// Direct reference to OverlayBdManager for import operations
    /// (shared Arc with the OverlayBD storage backend)
    overlaybd_manager: Option<Arc<OverlayBdManager>>,
    /// Path to qarax-init used for OCI boot injection and preflight checks.
    qarax_init_binary: Option<PathBuf>,
    /// How long `stop_vm` waits for a guest to power off after pressing its
    /// power button before forcing it off.
    graceful_stop_timeout: Duration,
    /// Certificates for migrating VMs in and out over TLS; plain TCP without.
    migration_tls: Option<MigrationTlsDir>,
}

impl VmManager {
    /// Create a new VM manager with storage backends
    pub fn with_storage(
        runtime_dir: impl Into<PathBuf>,
        ch_binary: impl Into<PathBuf>,
        storage_backends: StorageBackendRegistry,
        overlaybd_manager: Option<Arc<OverlayBdManager>>,
        qarax_init_binary: Option<PathBuf>,
        graceful_stop_timeout: Duration,
        migration_tls: Option<MigrationTlsDir>,
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
            vms: std::sync::Mutex::new(HashMap::new()),
            storage_backends,
            overlaybd_manager,
            qarax_init_binary,
            graceful_stop_timeout,
            migration_tls,
        }
    }

    /// Whether migrations in and out of this node use TLS.
    pub fn has_migration_tls(&self) -> bool {
        self.migration_tls.is_some()
    }

    /// Get the OverlayBD manager if configured (used for import operations)
    pub fn overlaybd_manager(&self) -> Option<&Arc<OverlayBdManager>> {
        self.overlaybd_manager.as_ref()
    }

    pub fn qarax_init_binary(&self) -> Option<&Path> {
        self.qarax_init_binary.as_deref()
    }

    /// Get a storage backend by kind
    pub fn storage_backend(
        &self,
        kind: StoragePoolKind,
    ) -> Option<&Arc<dyn crate::storage::StorageBackend>> {
        self.storage_backends.get(kind)
    }

    /// Get the runtime directory path
    pub fn runtime_dir(&self) -> &std::path::Path {
        &self.runtime_dir
    }

    /// Get the path to the Cloud Hypervisor binary
    pub fn ch_binary(&self) -> &std::path::Path {
        &self.ch_binary
    }

    /// Wait for `vm_id`'s lock (see [`ManagedVm`]).
    async fn lock_vm(&self, vm_id: &str) -> Result<VmGuard, VmManagerError> {
        let not_found = || VmManagerError::VmNotFound(vm_id.to_string());
        let instance = self
            .vms
            .lock()
            .unwrap()
            .get(vm_id)
            .map(|vm| vm.instance.clone());
        let instance = instance.ok_or_else(not_found)?.lock_owned().await;
        // Deleted while this waited.
        OwnedMutexGuard::try_map(instance, Option::as_mut).map_err(|_| not_found())
    }

    /// Take `vm_id` out of the manager once no operation holds its lock; those
    /// waiting for the lock then find the VM gone.
    async fn remove_vm(&self, vm_id: &str) -> Option<VmInstance> {
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
}

impl Drop for VmManager {
    fn drop(&mut self) {
        info!("VmManager dropped, all VMs will be terminated");
    }
}

mod api;
mod config;
mod exec;
mod fork;
mod helpers;
mod hotplug;
mod lifecycle;
mod migration;
mod vmm_impl;

#[cfg(test)]
mod tests;
