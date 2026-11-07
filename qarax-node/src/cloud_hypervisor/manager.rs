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
    ExecVmResponse, MemoryConfig as ProtoMemoryConfig, NetConfig as ProtoNetConfig,
    NumaPlacement as ProtoNumaPlacement, PayloadConfig as ProtoPayloadConfig,
    RateLimiterConfig as ProtoRateLimiterConfig, RngConfig as ProtoRngConfig,
    TokenBucket as ProtoTokenBucket, VfioDeviceConfig as ProtoVfioDeviceConfig,
    VhostMode as ProtoVhostMode, VmConfig as ProtoVmConfig, VmState, VmStatus,
    VsockConfig as ProtoVsockConfig,
};
use crate::storage::StorageBackendRegistry;

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
    /// Host-side UNIX socket used for virtio-vsock guest-agent access
    vsock_socket_path: Option<PathBuf>,
    /// Which storage backend mapped a disk for this VM (if any).
    /// Used by delete_vm to call the correct backend's unmap().
    storage_backend_kind: Option<StoragePoolKind>,
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
    /// Storage backend registry for attach/detach/map/unmap operations
    storage_backends: StorageBackendRegistry,
    /// Direct reference to OverlayBdManager for import operations
    /// (shared Arc with the OverlayBD storage backend)
    overlaybd_manager: Option<Arc<OverlayBdManager>>,
    /// Path to qarax-init used for OCI boot injection and preflight checks.
    qarax_init_binary: Option<PathBuf>,
}

impl VmManager {
    /// Create a new VM manager
    pub fn new(runtime_dir: impl Into<PathBuf>, ch_binary: impl Into<PathBuf>) -> Self {
        Self::with_storage(
            runtime_dir,
            ch_binary,
            StorageBackendRegistry::new(),
            None,
            None,
        )
    }

    /// Create a new VM manager with storage backends
    pub fn with_storage(
        runtime_dir: impl Into<PathBuf>,
        ch_binary: impl Into<PathBuf>,
        storage_backends: StorageBackendRegistry,
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
            storage_backends,
            overlaybd_manager,
            qarax_init_binary,
        }
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
}

impl Drop for VmManager {
    fn drop(&mut self) {
        info!("VmManager dropped, all VMs will be terminated");
    }
}

mod api;
mod config;
mod exec;
mod helpers;
mod hotplug;
mod lifecycle;
mod migration;
mod vmm_impl;

#[cfg(test)]
mod tests;
