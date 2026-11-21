use super::*;

const MAX_GUEST_AGENT_HANDSHAKE_BYTES: u64 = 4096;
const MAX_GUEST_AGENT_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

/// Wire format for the guest exec response returned by the qarax-init agent.
#[derive(serde::Deserialize)]
struct GuestExecResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

impl VmManager {
    fn exec_timeout(timeout_secs: Option<u64>) -> Duration {
        // Add a grace period so the guest's inner timeout fires first and returns
        // timed_out=true rather than racing with this outer tokio::time::timeout.
        const GRACE_SECS: u64 = 5;
        const SAFETY_TIMEOUT_SECS: u64 = 300;
        Duration::from_secs(
            timeout_secs
                .map(|s| s + GRACE_SECS)
                .unwrap_or(SAFETY_TIMEOUT_SECS),
        )
    }

    pub(super) fn build_guest_exec_request(
        command: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<Vec<u8>, VmManagerError> {
        #[derive(serde::Serialize)]
        struct GuestExecRequest {
            command: Vec<String>,
            timeout_secs: Option<u64>,
        }

        let mut request_json = serde_json::to_vec(&GuestExecRequest {
            command,
            timeout_secs,
        })
        .map_err(|e| VmManagerError::ExecInvalid(e.to_string()))?;
        request_json.push(b'\n');
        Ok(request_json)
    }

    /// The host end of `vm_id`'s guest-agent vsock, while the VM runs.
    pub(super) async fn guest_agent_socket(&self, vm_id: &str) -> Result<PathBuf, VmManagerError> {
        let instance = self.lock_vm(vm_id).await?;
        if instance.status() != VmStatus::Running {
            return Err(VmManagerError::ExecUnavailable(format!(
                "VM {} is not running",
                vm_id
            )));
        }
        instance
            .vsock_socket_path
            .clone()
            .ok_or_else(|| VmManagerError::ExecUnavailable(vm_id.to_string()))
    }

    /// Execute a command inside the guest through the sandbox exec guest agent.
    pub async fn exec_vm(
        &self,
        vm_id: &str,
        command: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<ExecVmResponse, VmManagerError> {
        if command.is_empty() {
            return Err(VmManagerError::ExecInvalid(
                "command must contain at least one argument".into(),
            ));
        }

        let vsock_socket_path = self.guest_agent_socket(vm_id).await?;

        let request_json = Self::build_guest_exec_request(command, timeout_secs)?;

        let timeout = Self::exec_timeout(timeout_secs);
        let response_bytes = tokio::time::timeout(timeout, async {
            let mut stream = tokio::net::UnixStream::connect(&vsock_socket_path)
                .await
                .map_err(|e| {
                    VmManagerError::ExecError(format!(
                        "failed to connect to guest-agent socket {}: {}",
                        vsock_socket_path.display(),
                        e
                    ))
                })?;

            stream.write_all(b"CONNECT 7000\n").await.map_err(|e| {
                VmManagerError::ExecError(format!(
                    "failed to open vsock stream to guest agent: {}",
                    e
                ))
            })?;

            let mut handshake = Vec::new();
            {
                let mut reader = tokio::io::BufReader::new(&mut stream);
                let mut limited = (&mut reader).take(MAX_GUEST_AGENT_HANDSHAKE_BYTES + 1);
                limited
                    .read_until(b'\n', &mut handshake)
                    .await
                    .map_err(|e| {
                        VmManagerError::ExecError(format!(
                            "failed to read guest-agent handshake: {}",
                            e
                        ))
                    })?;
            }

            if !handshake.ends_with(b"\n")
                || handshake.len() > MAX_GUEST_AGENT_HANDSHAKE_BYTES as usize
            {
                return Err(VmManagerError::ExecError(format!(
                    "guest-agent handshake exceeds {} bytes or is missing a newline terminator",
                    MAX_GUEST_AGENT_HANDSHAKE_BYTES
                )));
            }

            let handshake = String::from_utf8(handshake).map_err(|e| {
                VmManagerError::ExecError(format!(
                    "guest-agent handshake was not valid UTF-8: {}",
                    e
                ))
            })?;
            if !handshake.starts_with("OK") {
                return Err(VmManagerError::ExecError(format!(
                    "guest-agent connect failed: {}",
                    handshake.trim_end()
                )));
            }

            stream.write_all(&request_json).await.map_err(|e| {
                VmManagerError::ExecError(format!(
                    "failed to send exec request to guest agent: {}",
                    e
                ))
            })?;

            let mut response = Vec::new();
            stream
                .take(MAX_GUEST_AGENT_RESPONSE_BYTES + 1)
                .read_to_end(&mut response)
                .await
                .map_err(|e| {
                    VmManagerError::ExecError(format!("failed to read guest-agent response: {}", e))
                })?;
            if response.len() > MAX_GUEST_AGENT_RESPONSE_BYTES as usize {
                return Err(VmManagerError::ExecError(format!(
                    "guest-agent response exceeds {} bytes",
                    MAX_GUEST_AGENT_RESPONSE_BYTES
                )));
            }
            Ok::<_, VmManagerError>(response)
        })
        .await
        .map_err(|_| VmManagerError::ExecTimeout(timeout.as_secs()))??;

        let response: GuestExecResponse = serde_json::from_slice(&response_bytes).map_err(|e| {
            VmManagerError::ExecError(format!("invalid guest-agent response: {}", e))
        })?;

        Ok(ExecVmResponse {
            exit_code: response.exit_code,
            stdout: response.stdout,
            stderr: response.stderr,
            timed_out: response.timed_out,
        })
    }

    /// Get the PTY path for a VM's serial or console device.
    /// Returns (serial_pty_path, console_pty_path) if available.
    pub async fn get_pty_paths(
        &self,
        vm_id: &str,
    ) -> Result<(Option<String>, Option<String>), VmManagerError> {
        let instance = self.lock_vm(vm_id).await?;
        let config = instance.config();

        let serial_pty = if config
            .serial
            .as_ref()
            .map(|s| s.mode == ProtoConsoleMode::Pty as i32)
            .unwrap_or(false)
        {
            instance.serial_pty_path.clone()
        } else {
            None
        };

        let console_pty = if config
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

    /// Get the serial console PTY path if available.
    ///
    /// If the path was not discovered at create/start time (e.g. for recovered
    /// VMs), queries Cloud Hypervisor's vm.info API to obtain it on demand and
    /// caches the result in the instance for subsequent calls.
    pub async fn get_serial_pty_path(&self, vm_id: &str) -> Result<Option<String>, VmManagerError> {
        // Fast path: return cached value if available.
        let (socket_path, proto_config) = {
            let instance = self.lock_vm(vm_id).await?;

            if let Some(path) = &instance.serial_pty_path {
                return Ok(Some(path.clone()));
            }

            (instance.socket_path.clone(), instance.config())
        };

        let (pty_path, _) = self.query_pty_paths(&socket_path, &proto_config).await;

        // Cache the result so subsequent calls don't re-query the API
        if let Some(path) = &pty_path {
            info!("Discovered serial PTY path via vm.info: {}", path);
            if let Ok(mut instance) = self.lock_vm(vm_id).await {
                instance.serial_pty_path = Some(path.clone());
                let mut config = instance.config();
                if !config
                    .serial
                    .as_ref()
                    .map(|serial| serial.mode == ProtoConsoleMode::Pty as i32)
                    .unwrap_or(false)
                {
                    config.serial = Some(ProtoConsoleConfig {
                        mode: ProtoConsoleMode::Pty as i32,
                        file: None,
                        socket: None,
                        iommu: None,
                    });
                    instance.set_config(config);
                }
            }
        }

        Ok(pty_path)
    }

    /// Get the console device PTY path if available
    pub async fn get_console_pty_path(
        &self,
        vm_id: &str,
    ) -> Result<Option<String>, VmManagerError> {
        let instance = self.lock_vm(vm_id).await?;

        // Check if console is configured in PTY mode
        if let Some(console) = &instance.config().console
            && console.mode == ProtoConsoleMode::Pty as i32
        {
            return Ok(instance.console_pty_path.clone());
        }

        Ok(None)
    }
}
