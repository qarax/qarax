use super::*;

impl VmManager {
    pub(super) fn socket_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.sock", vm_id))
    }

    pub(super) fn cloud_init_seed_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}-cidata.img", vm_id))
    }

    pub(super) fn vsock_socket_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.vsock", vm_id))
    }

    pub(super) fn log_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.log", vm_id))
    }

    pub(super) fn config_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.json", vm_id))
    }

    /// Where Cloud Hypervisor's event monitor writes, see
    /// [`Self::spawn_cloud_hypervisor`].
    pub(super) fn events_path(&self, vm_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{}.events", vm_id))
    }

    /// Persist a VM's proto config for recovery after a node restart.
    pub(super) async fn persist_config(&self, vm_id: &str, config: &ProtoVmConfig) {
        if let Err(e) = tokio::fs::write(self.config_path(vm_id), config.encode_to_vec()).await {
            warn!("Failed to persist config for VM {}: {}", vm_id, e);
        }
    }

    pub(super) fn next_vsock_cid() -> i64 {
        use std::sync::atomic::{AtomicI64, Ordering};

        static NEXT_VSOCK_CID: AtomicI64 = AtomicI64::new(0x4000);
        NEXT_VSOCK_CID.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn resolve_vsock_config(&self, vm_id: &str, vsock: &mut ProtoVsockConfig) {
        if vsock.cid.is_none() {
            vsock.cid = Some(Self::next_vsock_cid());
        }
        if vsock.socket.is_none() {
            vsock.socket = Some(self.vsock_socket_path(vm_id).display().to_string());
        }
    }

    pub(super) fn vsock_socket_path_from_config(
        vsock: &Option<ProtoVsockConfig>,
    ) -> Option<PathBuf> {
        vsock
            .as_ref()
            .and_then(|cfg| cfg.socket.as_ref())
            .map(PathBuf::from)
    }

    /// Extract the first 8 hex digits from a VM UUID string (dashes stripped).
    pub(super) fn vm_hex_prefix(vm_id: &str) -> String {
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
    pub(super) fn tap_name_for_net(vm_id: &str, net_index: usize) -> String {
        format!("qt{}n{}", Self::vm_hex_prefix(vm_id), net_index)
    }

    pub(super) fn passt_socket_path(&self, vm_id: &str, net_index: usize) -> PathBuf {
        self.runtime_dir.join(format!(
            "qp{}n{}.sock",
            Self::vm_hex_prefix(vm_id),
            net_index
        ))
    }

    pub(super) fn should_spawn_passt(net: &ProtoNetConfig) -> bool {
        net.vhost_user.unwrap_or(false) && net.vhost_socket.as_deref() == Some("passt")
    }

    pub(super) async fn start_passt_backend(socket_path: &Path) -> Result<Child, VmManagerError> {
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

    pub(super) async fn cleanup_passt_processes(processes: &mut Vec<Child>) {
        for process in processes.iter_mut() {
            if let Err(e) = process.kill().await {
                warn!("Failed to kill passt process: {}", e);
            }
        }
        processes.clear();
    }

    /// Create a TAP device and bring it up.
    /// Wait for a Cloud Hypervisor process just spawned to listen on its API
    /// socket.
    pub(super) async fn wait_for_api_socket(socket_path: &Path) -> Result<(), VmManagerError> {
        const RETRIES: u32 = 50;
        let mut retries = 0;
        loop {
            match UnixStream::connect(socket_path).await {
                Ok(_) => return Ok(()),
                Err(_) if retries < RETRIES => {
                    retries += 1;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => return Err(VmManagerError::SpawnError(e)),
            }
        }
    }

    pub(super) async fn create_tap_device(name: &str) -> Result<(), VmManagerError> {
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
    pub(super) async fn delete_tap_device(name: &str) {
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

    /// Query Cloud Hypervisor's vm.info API to obtain PTY device paths.
    ///
    /// When a serial or console device is configured in PTY mode, CH allocates
    /// a PTY and exposes the slave device path in the vm.info response under
    /// `config.serial.file` / `config.console.file`. This is more reliable than
    /// log parsing because CH doesn't necessarily log the PTY path at all log levels.
    pub(super) async fn query_pty_paths(
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

    /// Start the Cloud Hypervisor process for `vm_id`, serving its API on
    /// [`Self::socket_path`], with stdout and stderr going to `log_file`.
    ///
    /// `--no-shutdown` keeps the VMM, and the VM defined in it, when the guest
    /// powers off, the same state `vm.shutdown` leaves: `vm.info` reports it as
    /// `Created` and `vm.boot` starts it again. Without it the process exits
    /// with the guest, and the VM can't be started until it is recreated.
    ///
    /// The event monitor is the only place Cloud Hypervisor reports how a
    /// live migration out ended (see [`Self::send_migration`]).
    pub(super) fn spawn_cloud_hypervisor(
        &self,
        vm_id: &str,
        log_file: std::fs::File,
    ) -> std::io::Result<Child> {
        let stderr_file = log_file.try_clone()?;
        let mut event_monitor = std::ffi::OsString::from("path=");
        event_monitor.push(self.events_path(vm_id));
        Command::new(&self.ch_binary)
            .arg("--api-socket")
            .arg(self.socket_path(vm_id))
            .arg("--no-shutdown")
            .arg("--event-monitor")
            .arg(event_monitor)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .kill_on_drop(true)
            .spawn()
    }

    /// The VM's state as Cloud Hypervisor reports it in `vm.info`.
    pub(super) async fn query_vm_state(
        socket_path: &PathBuf,
    ) -> Result<models::VmState, VmManagerError> {
        #[derive(serde::Deserialize)]
        struct VmInfoState {
            state: models::VmState,
        }

        let body = Self::send_api_request(socket_path, "GET", "/api/v1/vm.info", None).await?;
        let info: VmInfoState = serde_json::from_str(&body)
            .map_err(|e| VmManagerError::ProcessError(format!("unexpected vm.info: {e}")))?;
        Ok(info.state)
    }

    /// Whether the Cloud Hypervisor serving `socket_path` was started with
    /// `--no-shutdown`. Every process this node starts is (see
    /// [`Self::spawn_cloud_hypervisor`]), but one that survived a restart from
    /// an older qarax-node may not be: powering its guest off would end the
    /// process, so the VM could not be started again.
    pub(super) async fn keeps_vm_after_guest_power_off(socket_path: &PathBuf) -> bool {
        let ping = match Self::send_api_request(socket_path, "GET", "/api/v1/vmm.ping", None)
            .await
            .and_then(|body| {
                serde_json::from_str::<models::VmmPingResponse>(&body)
                    .map_err(|e| VmManagerError::ProcessError(format!("unexpected vmm.ping: {e}")))
            }) {
            Ok(ping) => ping,
            Err(e) => {
                warn!(
                    "Cloud Hypervisor at {} did not answer vmm.ping: {e}",
                    socket_path.display()
                );
                return false;
            }
        };
        let Some(pid) = ping.pid else {
            warn!(
                "Cloud Hypervisor at {} did not report its PID",
                socket_path.display()
            );
            return false;
        };
        match tokio::fs::read(format!("/proc/{pid}/cmdline")).await {
            Ok(cmdline) => cmdline
                .split(|b| *b == 0)
                .any(|arg| arg == b"--no-shutdown"),
            Err(e) => {
                warn!("Failed to read the command line of Cloud Hypervisor PID {pid}: {e}");
                false
            }
        }
    }
}
