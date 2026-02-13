use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info};

use crate::cloud_hypervisor::VmManager;
use crate::rpc::node::{
    AddDiskDeviceRequest, AddNetworkDeviceRequest, RemoveDeviceRequest, VmConfig, VmId, VmList,
    VmState, vm_service_server::VmService,
};

/// Implementation of VmService using Cloud Hypervisor
#[derive(Clone)]
pub struct VmServiceImpl {
    manager: Arc<VmManager>,
}

impl VmServiceImpl {
    /// Create a new VmServiceImpl with default paths
    pub fn new() -> Self {
        Self::with_paths("/var/lib/qarax/vms", "/usr/local/bin/cloud-hypervisor")
    }

    /// Create a new VmServiceImpl with custom paths
    pub fn with_paths(
        runtime_dir: impl Into<std::path::PathBuf>,
        ch_binary: impl Into<std::path::PathBuf>,
    ) -> Self {
        let manager = Arc::new(VmManager::new(runtime_dir, ch_binary));
        Self { manager }
    }

    /// Create from an existing VmManager
    pub fn from_manager(manager: Arc<VmManager>) -> Self {
        Self { manager }
    }
}

impl Default for VmServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl VmService for VmServiceImpl {
    async fn create_vm(&self, request: Request<VmConfig>) -> Result<Response<VmState>, Status> {
        let config = request.into_inner();
        let vm_id = config.vm_id.clone();

        info!("Creating VM: {}", vm_id);
        debug!("VM config: {:?}", config);

        match self.manager.create_vm(config).await {
            Ok(state) => {
                info!("VM {} created successfully", vm_id);
                Ok(Response::new(state))
            }
            Err(e) => {
                error!("Failed to create VM {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn start_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Starting VM: {}", vm_id);

        match self.manager.start_vm(&vm_id).await {
            Ok(()) => {
                info!("VM {} started successfully", vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to start VM {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn stop_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Stopping VM: {}", vm_id);

        match self.manager.stop_vm(&vm_id).await {
            Ok(()) => {
                info!("VM {} stopped successfully", vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to stop VM {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn pause_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Pausing VM: {}", vm_id);

        match self.manager.pause_vm(&vm_id).await {
            Ok(()) => {
                info!("VM {} paused successfully", vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to pause VM {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn resume_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Resuming VM: {}", vm_id);

        match self.manager.resume_vm(&vm_id).await {
            Ok(()) => {
                info!("VM {} resumed successfully", vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to resume VM {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn delete_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Deleting VM: {}", vm_id);

        match self.manager.delete_vm(&vm_id).await {
            Ok(()) => {
                info!("VM {} deleted successfully", vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to delete VM {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn get_vm_info(&self, request: Request<VmId>) -> Result<Response<VmState>, Status> {
        let vm_id = request.into_inner().id;
        info!("Getting VM info: {}", vm_id);

        match self.manager.get_vm_info(&vm_id).await {
            Ok(state) => Ok(Response::new(state)),
            Err(e) => {
                error!("Failed to get VM info {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn list_vms(&self, _request: Request<()>) -> Result<Response<VmList>, Status> {
        info!("Listing VMs");

        let vms = self.manager.list_vms().await;
        info!("Found {} VMs", vms.len());
        Ok(Response::new(VmList { vms }))
    }

    async fn add_network_device(
        &self,
        request: Request<AddNetworkDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Adding network device to VM: {}", req.vm_id);

        let config = req
            .config
            .ok_or_else(|| Status::invalid_argument("Missing network config"))?;

        match self.manager.add_network_device(&req.vm_id, &config).await {
            Ok(()) => {
                info!("Network device added to VM {}", req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to add network device to VM {}: {}", req.vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn remove_network_device(
        &self,
        request: Request<RemoveDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!(
            "Removing network device {} from VM: {}",
            req.device_id, req.vm_id
        );

        match self
            .manager
            .remove_network_device(&req.vm_id, &req.device_id)
            .await
        {
            Ok(()) => {
                info!(
                    "Network device {} removed from VM {}",
                    req.device_id, req.vm_id
                );
                Ok(Response::new(()))
            }
            Err(e) => {
                error!(
                    "Failed to remove network device {} from VM {}: {}",
                    req.device_id, req.vm_id, e
                );
                Err(map_manager_error(e))
            }
        }
    }

    async fn add_disk_device(
        &self,
        request: Request<AddDiskDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Adding disk device to VM: {}", req.vm_id);

        let config = req
            .config
            .ok_or_else(|| Status::invalid_argument("Missing disk config"))?;

        match self.manager.add_disk_device(&req.vm_id, &config).await {
            Ok(()) => {
                info!("Disk device added to VM {}", req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to add disk device to VM {}: {}", req.vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn remove_disk_device(
        &self,
        request: Request<RemoveDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!(
            "Removing disk device {} from VM: {}",
            req.device_id, req.vm_id
        );

        match self
            .manager
            .remove_disk_device(&req.vm_id, &req.device_id)
            .await
        {
            Ok(()) => {
                info!(
                    "Disk device {} removed from VM {}",
                    req.device_id, req.vm_id
                );
                Ok(Response::new(()))
            }
            Err(e) => {
                error!(
                    "Failed to remove disk device {} from VM {}: {}",
                    req.device_id, req.vm_id, e
                );
                Err(map_manager_error(e))
            }
        }
    }
}

fn map_manager_error(e: crate::cloud_hypervisor::VmManagerError) -> Status {
    use crate::cloud_hypervisor::VmManagerError;

    match e {
        VmManagerError::VmNotFound(id) => Status::not_found(format!("VM {} not found", id)),
        VmManagerError::VmAlreadyExists(id) => {
            Status::already_exists(format!("VM {} already exists", id))
        }
        VmManagerError::InvalidConfig(msg) => Status::invalid_argument(msg),
        VmManagerError::SpawnError(e) => Status::internal(format!("Failed to spawn CH: {}", e)),
        VmManagerError::SdkError(e) => {
            Status::internal(format!("Cloud Hypervisor SDK error: {}", e))
        }
        VmManagerError::ProcessError(msg) => Status::internal(msg),
    }
}
