use super::*;

impl VmManager {
    /// Add a network device to a VM
    pub async fn add_network_device(
        &self,
        vm_id: &str,
        config: &ProtoNetConfig,
    ) -> Result<(), VmManagerError> {
        let vms = self.vms.lock().await;
        let instance = vms
            .get(vm_id)
            .ok_or_else(|| VmManagerError::VmNotFound(vm_id.to_string()))?;

        let sdk_config = Self::proto_net_to_sdk(config)?;
        let body = serde_json::to_string(&sdk_config)
            .map_err(|e| VmManagerError::InvalidConfig(e.to_string()))?;

        Self::send_api_request(
            &instance.socket_path,
            "PUT",
            "/api/v1/vm.add-net",
            Some(&body),
        )
        .await?;

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

    /// Remove a network device from a VM
    pub async fn remove_network_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), VmManagerError> {
        self.remove_device_by_id(vm_id, device_id).await
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

    /// Resize the backing file for a disk (VM must be stopped).
    /// Uses fallocate to extend the file without filling it; falls back to truncate on NFS.
    pub async fn resize_disk(
        &self,
        _vm_id: &str,
        _disk_id: &str,
        path: &str,
        new_size: i64,
    ) -> Result<(), VmManagerError> {
        if path.is_empty() || path.contains('\0') {
            return Err(VmManagerError::InvalidConfig(
                "disk path is empty or contains null bytes".into(),
            ));
        }

        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| VmManagerError::StorageError(format!("stat {path}: {e}")))?;
        let current_size = metadata.len() as i64;
        if new_size <= current_size {
            return Err(VmManagerError::InvalidConfig(format!(
                "new_size {new_size} must be greater than current size {current_size}"
            )));
        }

        let status = tokio::process::Command::new("fallocate")
            .args(["-l", &new_size.to_string(), path])
            .status()
            .await
            .map_err(|e| VmManagerError::StorageError(e.to_string()))?;

        if !status.success() {
            // Fallback: truncate (always works, may create sparse regions)
            let status = tokio::process::Command::new("truncate")
                .args(["-s", &new_size.to_string(), path])
                .status()
                .await
                .map_err(|e| VmManagerError::StorageError(e.to_string()))?;
            if !status.success() {
                return Err(VmManagerError::StorageError(format!(
                    "both fallocate and truncate failed on {path}"
                )));
            }
        }

        Ok(())
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
