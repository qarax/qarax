pub mod block;
pub mod local;
pub mod nfs;
pub mod overlaybd;

use std::collections::HashMap;
use std::sync::Arc;

use crate::rpc::node::StoragePoolKind;

/// Result of mapping a storage object to a device/path on the host.
#[derive(Debug, Clone)]
pub struct MappedDisk {
    /// Path usable by Cloud Hypervisor (e.g., "/dev/sdb" or "/var/lib/qarax/pools/.../disk.raw")
    pub device_path: String,
}

/// Abstraction over storage backends (Local, NFS, OverlayBD, etc).
///
/// Each backend knows how to make storage accessible on a host (attach/detach)
/// and how to present a specific image/disk as a device path for a VM (map/unmap).
#[tonic::async_trait]
pub trait StorageBackend: Send + Sync {
    /// Make this pool accessible on the host.
    async fn attach(&self, pool_id: &str, config_json: &str) -> anyhow::Result<String>;

    /// Reverse of attach.
    async fn detach(&self, pool_id: &str, config_json: &str) -> anyhow::Result<()>;

    /// Present a storage object as a device/path for a specific VM.
    async fn map(&self, vm_id: &str, config: &serde_json::Value) -> anyhow::Result<MappedDisk>;

    /// Release the mapping created by map().
    async fn unmap(&self, vm_id: &str) -> anyhow::Result<()>;

    /// Rebuild internal state after a restart.
    async fn recover(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Registry of storage backends keyed by pool kind.
#[derive(Default)]
pub struct StorageBackendRegistry {
    backends: HashMap<i32, Arc<dyn StorageBackend>>,
}

impl StorageBackendRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, kind: StoragePoolKind, backend: Arc<dyn StorageBackend>) {
        self.backends.insert(kind as i32, backend);
    }

    pub fn get(&self, kind: StoragePoolKind) -> Option<&Arc<dyn StorageBackend>> {
        self.backends.get(&(kind as i32))
    }
}
