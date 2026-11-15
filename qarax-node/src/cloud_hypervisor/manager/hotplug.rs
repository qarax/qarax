use super::*;

impl VmManager {
    /// Add a network device to a VM. A NIC without a TAP gets one on its
    /// bridge, as at create time; Cloud Hypervisor would otherwise make its
    /// own, off the bridge and holding the guest's address on the host side.
    /// The NIC joins the persisted config, so recovery and the node's
    /// per-NIC networking (DHCP, firewall) see it.
    pub async fn add_network_device(
        &self,
        vm_id: &str,
        config: &ProtoNetConfig,
    ) -> Result<(), VmManagerError> {
        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let mut net = config.clone();
        let created_tap = if !net.vhost_user.unwrap_or(false) && net.tap.is_none() {
            let in_use: Vec<&str> = instance
                .proto_config
                .networks
                .iter()
                .filter_map(|n| n.tap.as_deref())
                .collect();
            let tap = (0..)
                .map(|i| Self::tap_name_for_net(vm_id, i))
                .find(|name| !in_use.contains(&name.as_str()))
                .expect("an unused TAP index exists");
            Self::create_tap_device(&tap).await?;
            net.tap = Some(tap.clone());
            Some(tap)
        } else {
            None
        };

        let result = async {
            if let (Some(tap), Some(bridge)) = (&net.tap, &net.bridge) {
                crate::networking::bridge::attach_to_bridge(tap, bridge)
                    .await
                    .map_err(|e| {
                        VmManagerError::TapError(format!(
                            "Failed to attach TAP {tap} to bridge {bridge}: {e}"
                        ))
                    })?;
            }
            let sdk_config = Self::proto_net_to_sdk(&net)?;
            let body = serde_json::to_string(&sdk_config)
                .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;
            Self::send_api_request(
                &instance.socket_path,
                "PUT",
                "/api/v1/vm.add-net",
                Some(&body),
            )
            .await
            .map(|_| ())
        }
        .await;
        if let Err(e) = result {
            if let Some(tap) = &created_tap {
                Self::delete_tap_device(tap).await;
            }
            return Err(e);
        }

        instance.proto_config.networks.push(net);
        instance.tap_devices.extend(created_tap);
        self.persist_config(vm_id, &instance.proto_config).await;
        Ok(())
    }

    async fn remove_device_by_id(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), VmManagerError> {
        let socket_path = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
            instance.socket_path.clone()
        };

        let body = serde_json::json!({ "id": device_id }).to_string();
        Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.remove-device", Some(&body))
            .await?;
        Ok(())
    }

    /// Remove a network device from a VM, with the TAP qarax-node made for it.
    pub async fn remove_network_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), VmManagerError> {
        self.remove_device_by_id(vm_id, device_id).await?;

        let mut vms = self.vms.lock().await;
        let instance = vms
            .get_mut(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
        let Some(index) = instance
            .proto_config
            .networks
            .iter()
            .position(|n| n.id == device_id)
        else {
            return Ok(());
        };
        let net = instance.proto_config.networks.remove(index);
        if let Some(tap) = net.tap
            && instance.tap_devices.contains(&tap)
        {
            Self::delete_tap_device(&tap).await;
            instance.tap_devices.retain(|t| *t != tap);
        }
        self.persist_config(vm_id, &instance.proto_config).await;
        Ok(())
    }

    /// Add a disk device to a VM
    pub async fn add_disk_device(
        &self,
        vm_id: &str,
        config: &ProtoDiskConfig,
    ) -> Result<(), VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let sdk_config = Self::proto_disk_to_sdk(config)?;
        let body = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        Self::send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.add-disk",
            Some(&body),
        )
        .await?;

        Ok(())
    }

    /// Remove a disk device from a VM
    pub async fn remove_disk_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), VmManagerError> {
        self.remove_device_by_id(vm_id, device_id).await
    }

    /// Resize vCPUs and/or memory of a running VM
    pub async fn resize_vm(
        &self,
        vm_id: &str,
        desired_vcpus: Option<i32>,
        desired_ram: Option<i64>,
    ) -> Result<(), VmManagerError> {
        let socket_path = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;
            instance.socket_path.clone()
        };

        #[derive(serde::Serialize)]
        struct VmResizeBody {
            #[serde(skip_serializing_if = "Option::is_none")]
            desired_vcpus: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            desired_ram: Option<i64>,
        }

        let body = serde_json::to_string(&VmResizeBody {
            desired_vcpus,
            desired_ram,
        })
        .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.resize", Some(&body)).await?;

        Ok(())
    }

    /// Grow disk `disk_id`, whose image is at `path`, to `new_size` bytes.
    ///
    /// A booted VM's disk grows through Cloud Hypervisor, which resizes the
    /// image and tells the guest. Otherwise the image grows in place with
    /// `qemu-img`, which also updates the virtual size in a qcow2 header;
    /// growing the file alone would leave a qcow2 disk its old size. The new
    /// space is sparse either way.
    pub async fn resize_disk(
        &self,
        vm_id: &str,
        disk_id: &str,
        path: &str,
        new_size: i64,
        image_format: ProtoImageFormat,
    ) -> Result<(), VmManagerError> {
        if path.is_empty() || path.contains('\0') {
            return Err(VmManagerError::InvalidConfig(
                "disk path is empty or contains null bytes".into(),
            ));
        }

        let socket_path = {
            let vms = self.vms.lock().await;
            vms.get(vm_id)
                .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?
                .socket_path
                .clone()
        };
        if matches!(
            Self::query_vm_state(&socket_path).await,
            Ok(models::VmState::Running | models::VmState::Paused)
        ) {
            let body = serde_json::to_string(&models::VmResizeDisk {
                id: Some(disk_id.to_string()),
                desired_size: Some(new_size),
            })
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;
            Self::send_api_request(&socket_path, "PUT", "/api/v1/vm.resize-disk", Some(&body))
                .await?;
            return Ok(());
        }

        crate::disk_image::grow_image(Path::new(path), new_size, image_format)
            .await
            .map_err(VmManagerError::StorageError)
    }

    /// Add a VFIO device (e.g., GPU) to a running VM
    pub async fn add_device(
        &self,
        vm_id: &str,
        config: &ProtoVfioDeviceConfig,
    ) -> Result<(), VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let sdk_config = Self::proto_vfio_device_to_sdk(config)?;
        let body = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        Self::send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.add-device",
            Some(&body),
        )
        .await?;

        Ok(())
    }

    /// Remove a VFIO device from a running VM
    pub async fn remove_device(&self, vm_id: &str, device_id: &str) -> Result<(), VmManagerError> {
        self.remove_device_by_id(vm_id, device_id).await
    }
}
