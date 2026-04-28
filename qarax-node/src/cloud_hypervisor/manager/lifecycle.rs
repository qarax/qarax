use super::*;

impl VmManager {
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

            let vsock_socket_path = Self::vsock_socket_path_from_config(&proto_config.vsock);

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
                vsock_socket_path,
                storage_backend_kind: None, // Recovery doesn't restore OverlayBD state
            };

            let mut vms = self.vms.lock().await;
            vms.insert(vm_id.clone(), instance);
            info!("Recovered VM {} with status {:?}", vm_id, status);
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
        if let Some(vsock) = config.vsock.as_mut() {
            self.resolve_vsock_config(&vm_id, vsock);
        }
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

        // Resolve storage-backed disks: map each disk that has oci_image_ref set
        // through the appropriate storage backend.
        let mut storage_backend_kind = None;
        for disk in config.disks.iter_mut() {
            if let (Some(image_ref), Some(registry_url)) =
                (disk.oci_image_ref.clone(), disk.registry_url.clone())
            {
                let backend = self
                    .storage_backends
                    .get(StoragePoolKind::Overlaybd)
                    .ok_or_else(|| {
                        VmManagerError::StorageError(format!(
                            "Disk {} requests OverlayBD but no OverlayBD backend is configured",
                            disk.id
                        ))
                    })?;

                let mut disk_config = serde_json::json!({
                    "image_ref": image_ref,
                    "registry_url": registry_url,
                });
                if let Some(ref upper_data) = disk.upper_data_path {
                    disk_config["upper_data_path"] = serde_json::Value::String(upper_data.clone());
                }
                if let Some(ref upper_index) = disk.upper_index_path {
                    disk_config["upper_index_path"] =
                        serde_json::Value::String(upper_index.clone());
                }
                let mapped = backend
                    .map(&vm_id, &disk_config)
                    .await
                    .map_err(|e| VmManagerError::StorageError(e.to_string()))?;

                disk.path = Some(mapped.device_path);
                disk.oci_image_ref = None;
                disk.registry_url = None;
                storage_backend_kind = Some(StoragePoolKind::Overlaybd);
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
            let buf = super::super::cloud_init::build_seed_image(
                &ci.user_data,
                &ci.meta_data,
                network_config,
            )
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
        let vsock_socket_path = Self::vsock_socket_path_from_config(&config.vsock);
        let log_path = self.log_path(&vm_id);
        let config_path = self.config_path(&vm_id);

        // Remove old socket if it exists
        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }
        if let Some(path) = &vsock_socket_path
            && path.exists()
        {
            let _ = tokio::fs::remove_file(path).await;
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
            vsock_socket_path,
            storage_backend_kind,
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

    /// Force stop (hard power-off) a VM by killing the Cloud Hypervisor process.
    ///
    /// Unlike `stop_vm` (graceful shutdown), this immediately kills the CH process.
    /// Unlike `delete_vm`, this preserves all VM resources (TAP devices, sockets,
    /// configs) so the VM can be deleted or recreated later.
    pub async fn force_stop_vm(&self, vm_id: &str) -> Result<(), VmManagerError> {
        info!("Force stopping VM: {}", vm_id);

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        if let Some(mut process) = instance.process.take()
            && let Err(e) = process.kill().await
        {
            warn!("Failed to kill CH process for VM {}: {}", vm_id, e);
        }

        instance.status = VmStatus::Shutdown;

        info!("VM {} force stopped successfully", vm_id);
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
        if let Some(vsock_socket_path) = &instance.vsock_socket_path
            && vsock_socket_path.exists()
        {
            let _ = tokio::fs::remove_file(vsock_socket_path).await;
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

        if let Err(e) = crate::networking::iptables::teardown_vm_firewall(vm_id).await {
            warn!("Failed to tear down firewall state for VM {}: {}", vm_id, e);
        }

        // Stop passt backends created by qarax-node
        Self::cleanup_passt_processes(&mut instance.passt_processes).await;

        // Unmap storage backend device if this VM used one
        if let Some(kind) = instance.storage_backend_kind
            && let Some(backend) = self.storage_backends.get(kind)
            && let Err(e) = backend.unmap(vm_id).await
        {
            warn!("Failed to unmap storage for VM {}: {}", vm_id, e);
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
}
