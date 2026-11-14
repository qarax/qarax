use super::*;

impl VmManager {
    /// Snapshot a VM
    pub async fn snapshot_vm(&self, vm_id: &str, snapshot_url: &str) -> Result<(), VmManagerError> {
        info!("Snapshotting VM: {}", vm_id);
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
        let socket_path = instance.socket_path.clone();
        drop(vms);

        // Cloud Hypervisor requires the destination to be an existing directory.
        let dest_path = snapshot_url.trim_prefix("file://");
        tokio::fs::create_dir_all(dest_path).await.map_err(|e| {
            VmManagerError::ProcessError(format!(
                "Failed to create snapshot directory {}: {}",
                dest_path, e
            ))
        })?;

        let body = format!(r#"{{"destination_url":"{}"}}"#, snapshot_url);
        Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.snapshot", Some(&body)).await?;
        info!("VM {} snapshotted successfully to {}", vm_id, snapshot_url);
        Ok(())
    }

    /// Restore a VM from a snapshot.
    ///
    /// Spawns a fresh Cloud Hypervisor process for the given vm_id, then calls
    /// `PUT /api/v1/vm.restore` (without a preceding `vm.create`). Cloud Hypervisor
    /// reads all VM config from the snapshot, so no VmConfig is needed here.
    pub async fn restore_vm(&self, vm_id: &str, source_url: &str) -> Result<(), VmManagerError> {
        info!("Restoring VM {} from {}", vm_id, source_url);

        let config_path = self.config_path(vm_id);
        let proto_config = match tokio::fs::read(&config_path).await {
            Ok(config_bytes) => ProtoVmConfig::decode(config_bytes.as_slice()).map_err(|e| {
                VmManagerError::InvalidConfig(format!(
                    "Failed to decode persisted config for restored VM {}: {}",
                    vm_id, e
                ))
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProtoVmConfig {
                vm_id: vm_id.to_string(),
                serial: Some(ProtoConsoleConfig {
                    mode: ProtoConsoleMode::Pty as i32,
                    file: None,
                    socket: None,
                    iommu: None,
                }),
                ..Default::default()
            },
            Err(e) => return Err(VmManagerError::SpawnError(e)),
        };

        // Clean up any existing CH process for this vm_id.
        {
            let mut vms = self.vms.lock().await;
            if let Some(mut instance) = vms.remove(vm_id) {
                if let Some(mut process) = instance.process.take() {
                    let _ = process.kill().await;
                }
                if instance.socket_path.exists() {
                    let _ = tokio::fs::remove_file(&instance.socket_path).await;
                }
            }
        }

        // Ensure runtime directory exists.
        tokio::fs::create_dir_all(&self.runtime_dir)
            .await
            .map_err(VmManagerError::SpawnError)?;

        let socket_path = self.socket_path(vm_id);
        let log_path = self.log_path(vm_id);

        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }

        let log_file = tokio::fs::File::create(&log_path)
            .await
            .map_err(VmManagerError::SpawnError)?
            .into_std()
            .await;
        let stderr_file = log_file.try_clone().map_err(VmManagerError::SpawnError)?;

        let process = Command::new(&self.ch_binary)
            .arg("--api-socket")
            .arg(&socket_path)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .kill_on_drop(true)
            .spawn()
            .map_err(VmManagerError::SpawnError)?;

        info!(
            "Cloud Hypervisor process for restore started with PID: {:?}",
            process.id()
        );

        // Wait for socket to be ready.
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

        // Call vm.restore — Cloud Hypervisor reads all config from the snapshot.
        // After vm.restore, CH leaves the VM in paused state; vm.resume is required.
        let body = format!(r#"{{"source_url":"{}","prefault":false}}"#, source_url);
        if let Err(e) =
            Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.restore", Some(&body)).await
        {
            // Kill the CH process if restore fails.
            let _ = tokio::fs::remove_file(&socket_path).await;
            return Err(e);
        }

        if let Err(e) = Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.resume", None).await
        {
            let _ = tokio::fs::remove_file(&socket_path).await;
            return Err(e);
        }

        let (serial_pty, console_pty) = self.query_pty_paths(&socket_path, &proto_config).await;

        // Register the restored instance with the persisted proto config so
        // console/PTY metadata remains available after snapshot restore.
        let vm_uuid = Uuid::parse_str(vm_id)
            .map_err(|e| VmManagerError::InvalidConfig(format!("Invalid VM ID: {}", e)))?;

        let machine_config = MachineConfig {
            vm_id: vm_uuid,
            socket_path: Cow::Owned(socket_path.clone()),
            exec_path: Cow::Owned(self.ch_binary.clone()),
        };

        let vm = Machine::connect(machine_config)
            .await
            .map_err(VmManagerError::SdkError)?;

        let vsock_socket_path = Self::vsock_socket_path_from_config(&proto_config.vsock);

        let instance = VmInstance {
            proto_config,
            process: Some(process),
            vm,
            socket_path: socket_path.clone(),
            status: VmStatus::Running,
            tap_devices: vec![],
            passt_processes: vec![],
            serial_pty_path: serial_pty,
            console_pty_path: console_pty,
            vsock_socket_path,
            storage_backend_kind: None,
        };

        {
            let mut vms = self.vms.lock().await;
            vms.insert(vm_id.to_string(), instance);
        }

        info!("VM {} restored successfully from {}", vm_id, source_url);
        Ok(())
    }

    /// Prepare this node to receive a live migration for the given VM.
    ///
    /// Steps:
    /// 1. Create TAP devices for all networks in the supplied config.
    /// 2. Spawn a Cloud Hypervisor process.
    /// 3. Call `vm.receive-migration` on that process.
    /// 4. Register a placeholder VmInstance so the VM is tracked.
    ///
    /// Returns the `receiver_url` that the source node must pass to
    /// `vm.send-migration` (e.g. `"tcp:0.0.0.0:49152"`).
    pub async fn receive_migration(
        &self,
        vm_id: &str,
        config: ProtoVmConfig,
        migration_port: u16,
    ) -> Result<String, VmManagerError> {
        info!(
            "Preparing to receive migration for VM {} on port {}",
            vm_id, migration_port
        );

        {
            let vms = self.vms.lock().await;
            if vms.contains_key(vm_id) {
                return Err(VmManagerError::VmAlreadyExists(vm_id.to_string()));
            }
        }

        // Pick a free TCP port if the caller passed 0.
        let port = if migration_port == 0 {
            tokio::net::TcpListener::bind("0.0.0.0:0")
                .await
                .map_err(|e| {
                    VmManagerError::MigrationError(format!("Failed to bind ephemeral port: {}", e))
                })?
                .local_addr()
                .map_err(|e| {
                    VmManagerError::MigrationError(format!("Failed to get ephemeral port: {}", e))
                })?
                .port()
        } else {
            migration_port
        };

        // Create TAP devices for the incoming VM's networks.
        let mut tap_devices: Vec<String> = Vec::new();
        let mut mutable_config = config.clone();
        if let Some(vsock) = mutable_config.vsock.as_mut() {
            self.resolve_vsock_config(vm_id, vsock);
        }
        for (i, net) in mutable_config.networks.iter_mut().enumerate() {
            if !net.vhost_user.unwrap_or(false) && net.tap.is_none() {
                let tap_name = Self::tap_name_for_net(vm_id, i);
                if let Err(e) = Self::create_tap_device(&tap_name).await {
                    for tap in &tap_devices {
                        Self::delete_tap_device(tap).await;
                    }
                    return Err(e);
                }
                // Attach to bridge if specified.
                if let Some(bridge_name) = &net.bridge
                    && let Err(e) =
                        crate::networking::bridge::attach_to_bridge(&tap_name, bridge_name).await
                {
                    for tap in &tap_devices {
                        Self::delete_tap_device(tap).await;
                    }
                    return Err(VmManagerError::TapError(format!(
                        "Failed to attach TAP {} to bridge {}: {}",
                        tap_name, bridge_name, e
                    )));
                }
                net.tap = Some(tap_name.clone());
                tap_devices.push(tap_name);
            }
        }

        // Resolve storage-backed disks before spawning CH — the OverlayBD
        // backend needs to mount a TCMU device on this host for the same image
        // the source node was using.
        let mut storage_backend_kind = None;
        for disk in mutable_config.disks.iter_mut() {
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
                    .map(vm_id, &disk_config)
                    .await
                    .map_err(|e| VmManagerError::StorageError(e.to_string()))?;

                disk.path = Some(mapped.device_path);
                disk.oci_image_ref = None;
                disk.registry_url = None;
                storage_backend_kind = Some(StoragePoolKind::Overlaybd);
            }
        }

        // Ensure runtime directory exists.
        tokio::fs::create_dir_all(&self.runtime_dir)
            .await
            .map_err(VmManagerError::SpawnError)?;

        let socket_path = self.socket_path(vm_id);
        let vsock_socket_path = Self::vsock_socket_path_from_config(&mutable_config.vsock);
        let log_path = self.log_path(vm_id);

        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }
        if let Some(path) = &vsock_socket_path
            && path.exists()
        {
            let _ = tokio::fs::remove_file(path).await;
        }

        let log_file = match tokio::fs::File::create(&log_path).await {
            Ok(f) => f,
            Err(e) => {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                return Err(VmManagerError::SpawnError(e));
            }
        }
        .into_std()
        .await;

        let stderr_file = match log_file.try_clone() {
            Ok(f) => f,
            Err(e) => {
                for tap in &tap_devices {
                    Self::delete_tap_device(tap).await;
                }
                return Err(VmManagerError::SpawnError(e));
            }
        };

        let process = Command::new(&self.ch_binary)
            .arg("--api-socket")
            .arg(&socket_path)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .kill_on_drop(true)
            .spawn()
            .map_err(VmManagerError::SpawnError)?;

        info!(
            "Cloud Hypervisor receive-migration process started with PID: {:?}",
            process.id()
        );

        // Wait for the API socket to become available.
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
                    return Err(VmManagerError::SpawnError(e));
                }
            }
        }

        let receiver_url = format!("tcp:0.0.0.0:{}", port);

        self.persist_config(vm_id, &mutable_config).await;

        let vm_uuid = Uuid::parse_str(vm_id)
            .map_err(|e| VmManagerError::InvalidConfig(format!("Invalid VM ID: {}", e)))?;

        let machine_config = MachineConfig {
            vm_id: vm_uuid,
            socket_path: Cow::Owned(socket_path.clone()),
            exec_path: Cow::Owned(self.ch_binary.clone()),
        };

        let vm = Machine::connect(machine_config)
            .await
            .map_err(VmManagerError::SdkError)?;

        let instance = VmInstance {
            proto_config: mutable_config,
            process: Some(process),
            vm,
            socket_path: socket_path.clone(),
            status: VmStatus::Created,
            tap_devices,
            passt_processes: Vec::new(),
            serial_pty_path: None,
            console_pty_path: None,
            vsock_socket_path,
            storage_backend_kind,
        };

        {
            let mut vms = self.vms.lock().await;
            vms.insert(vm_id.to_string(), instance);
        }

        // vm.receive-migration blocks until the sender completes the full transfer.
        // Spawn it as a background task so we can return the receiver URL immediately;
        // the control plane will call send_migration on the source concurrently.
        let body = format!(r#"{{"receiver_url":"{}"}}"#, receiver_url);
        let socket_path_bg = socket_path.clone();
        let vm_id_bg = vm_id.to_string();
        tokio::spawn(async move {
            match Self::send_api_request(
                &socket_path_bg,
                "PUT",
                "/api/v1/vm.receive-migration",
                Some(&body),
            )
            .await
            {
                Ok(_) => info!("VM {} receive-migration completed", vm_id_bg),
                Err(e) => error!(
                    "VM {} receive-migration background task failed: {}",
                    vm_id_bg, e
                ),
            }
        });

        info!(
            "VM {} ready to receive migration on {}",
            vm_id, receiver_url
        );
        Ok(receiver_url)
    }

    /// Send a live migration from this node to the destination.
    ///
    /// Calls `vm.send-migration` on the source Cloud Hypervisor process.
    /// This call blocks until Cloud Hypervisor completes the migration.
    /// On success the source VM process has exited; the VmInstance is removed
    /// from the manager (TAP cleanup is left to the caller via `delete_vm` or
    /// an explicit cleanup step).
    pub async fn send_migration(
        &self,
        vm_id: &str,
        destination_url: &str,
    ) -> Result<(), VmManagerError> {
        info!("Sending migration for VM {} to {}", vm_id, destination_url);

        let socket_path = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
            instance.socket_path.clone()
        };

        let body = format!(r#"{{"destination_url":"{}"}}"#, destination_url);
        Self::send_api_request(
            &socket_path,
            "PUT",
            "/api/v1/vm.send-migration",
            Some(&body),
        )
        .await
        .map_err(|e| VmManagerError::MigrationError(format!("vm.send-migration failed: {}", e)))?;

        // Mark the source instance as Shutdown.  We keep it in the map so
        // the control plane can call delete_vm() to clean up TAP devices and
        // other host resources after confirming migration success.
        {
            let mut vms = self.vms.lock().await;
            if let Some(instance) = vms.get_mut(vm_id) {
                instance.status = VmStatus::Shutdown;
            }
        }

        info!(
            "VM {} migrated out successfully to {}",
            vm_id, destination_url
        );
        Ok(())
    }
}
