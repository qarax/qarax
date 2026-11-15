use std::sync::atomic::{AtomicI64, Ordering};

use super::*;

static NEXT_VSOCK_CID: AtomicI64 = AtomicI64::new(0x5000);

impl FirecrackerManager {
    pub(super) fn socket_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.fc.sock", vm_id))
    }

    pub(super) fn vsock_socket_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.fc.vsock", vm_id))
    }

    pub(super) fn log_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.fc.log", vm_id))
    }

    pub(super) fn config_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.fc.json", vm_id))
    }

    pub(super) fn cloud_init_seed_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}-fc-cidata.img", vm_id))
    }

    pub(super) fn next_vsock_cid() -> i64 {
        NEXT_VSOCK_CID.fetch_add(1, Ordering::Relaxed)
    }

    /// Ensure the CID counter is above `used_cid` so recovered VMs never collide
    /// with freshly assigned CIDs after a process restart.
    pub(super) fn advance_vsock_cid_past(used_cid: i64) {
        let next = used_cid + 1;
        let mut cur = NEXT_VSOCK_CID.load(Ordering::Relaxed);
        while cur < next {
            match NEXT_VSOCK_CID.compare_exchange_weak(
                cur,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
    }

    pub(super) fn resolve_vsock_config(
        &self,
        vm_id: &str,
        vsock: &mut crate::rpc::node::VsockConfig,
    ) {
        if vsock.cid.is_none() {
            vsock.cid = Some(Self::next_vsock_cid());
        }
        if vsock.socket.is_none() {
            vsock.socket = Some(self.vsock_socket_path(vm_id).display().to_string());
        }
    }

    pub(super) fn vsock_socket_path_from_config(
        vsock: &Option<crate::rpc::node::VsockConfig>,
    ) -> Option<PathBuf> {
        vsock
            .as_ref()
            .and_then(|cfg| cfg.socket.as_ref())
            .map(PathBuf::from)
    }

    pub(super) fn tap_name_for_net(vm_id: &str, net_index: usize) -> String {
        let hex: String = vm_id
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .take(8)
            .collect();
        format!("qf{}n{}", hex, net_index)
    }

    pub(super) async fn create_tap_device(name: &str) -> Result<(), VmmError> {
        let add = Command::new("ip")
            .args(["tuntap", "add", name, "mode", "tap"])
            .status()
            .await
            .map_err(|e| VmmError::TapError(format!("failed to run ip tuntap add: {e}")))?;
        if !add.success() {
            return Err(VmmError::TapError(format!(
                "ip tuntap add {name} failed with status {add}"
            )));
        }

        let up = Command::new("ip")
            .args(["link", "set", name, "up"])
            .status()
            .await
            .map_err(|e| VmmError::TapError(format!("failed to run ip link set up: {e}")))?;
        if !up.success() {
            return Err(VmmError::TapError(format!(
                "ip link set {name} up failed with status {up}"
            )));
        }

        info!("FC TAP device {} created and up", name);
        Ok(())
    }

    pub(super) async fn delete_tap_device(name: &str) {
        match Command::new("ip")
            .args(["link", "delete", name])
            .status()
            .await
        {
            Ok(s) if s.success() => info!("FC TAP device {} deleted", name),
            Ok(s) => warn!("ip link delete {} failed with status {}", name, s),
            Err(e) => warn!("Failed to run ip link delete {}: {}", name, e),
        }
    }

    pub(super) async fn load_persisted_config(
        &self,
        vm_id: &str,
    ) -> Result<Option<ProtoVmConfig>, VmmError> {
        let config_path = self.config_path(vm_id);
        if !config_path.exists() {
            return Ok(None);
        }
        let bytes = tokio::fs::read(&config_path)
            .await
            .map_err(VmmError::SpawnError)?;
        let config = ProtoVmConfig::decode(bytes.as_slice()).map_err(|e| {
            VmmError::InvalidConfig(format!("Failed to decode FC config for {}: {}", vm_id, e))
        })?;
        Ok(Some(config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_vsock_config_sets_defaults() {
        let runtime_dir = TempDir::new().unwrap();
        let manager =
            FirecrackerManager::new(runtime_dir.path(), "/bin/true", Duration::from_secs(1));
        let mut vsock = crate::rpc::node::VsockConfig::default();

        manager.resolve_vsock_config("test-vm", &mut vsock);

        assert!(vsock.cid.is_some());
        assert_eq!(
            vsock.socket.as_deref(),
            Some(
                runtime_dir
                    .path()
                    .join("test-vm.fc.vsock")
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }
}
