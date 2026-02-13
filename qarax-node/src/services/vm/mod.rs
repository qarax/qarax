use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tonic::{Request, Response, Status};
use tracing::{debug, info};

use crate::rpc::node::{
    AddDiskDeviceRequest, AddNetworkDeviceRequest, RemoveDeviceRequest, VmConfig, VmId, VmList,
    VmState, VmStatus, vm_service_server::VmService,
};

/// NOOP implementation of VmService for testing
/// This implementation stores VM state in memory but doesn't actually create VMs
#[derive(Debug, Default, Clone)]
pub struct VmServiceImpl {
    // In-memory storage of VM states
    vms: Arc<Mutex<HashMap<String, VmState>>>,
}

impl VmServiceImpl {
    pub fn new() -> Self {
        Self {
            vms: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[tonic::async_trait]
impl VmService for VmServiceImpl {
    async fn create_vm(&self, request: Request<VmConfig>) -> Result<Response<VmState>, Status> {
        let config = request.into_inner();
        let vm_id = config.vm_id.clone();

        info!("Creating VM: {}", vm_id);
        debug!("VM config: {:?}", config);

        // Create VM state
        let state = VmState {
            config: Some(config),
            status: VmStatus::Created.into(),
            memory_actual_size: None,
        };

        // Store in memory
        {
            let mut vms = self.vms.lock().unwrap();
            vms.insert(vm_id.clone(), state.clone());
        }

        info!("VM {} created successfully (NOOP)", vm_id);
        Ok(Response::new(state))
    }

    async fn start_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Starting VM: {}", vm_id);

        // Update status
        {
            let mut vms = self.vms.lock().unwrap();
            if let Some(state) = vms.get_mut(&vm_id) {
                state.status = VmStatus::Running.into();
                info!("VM {} started successfully (NOOP)", vm_id);
            } else {
                return Err(Status::not_found(format!("VM {} not found", vm_id)));
            }
        }

        Ok(Response::new(()))
    }

    async fn stop_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Stopping VM: {}", vm_id);

        // Update status
        {
            let mut vms = self.vms.lock().unwrap();
            if let Some(state) = vms.get_mut(&vm_id) {
                state.status = VmStatus::Shutdown.into();
                info!("VM {} stopped successfully (NOOP)", vm_id);
            } else {
                return Err(Status::not_found(format!("VM {} not found", vm_id)));
            }
        }

        Ok(Response::new(()))
    }

    async fn pause_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Pausing VM: {}", vm_id);

        // Update status
        {
            let mut vms = self.vms.lock().unwrap();
            if let Some(state) = vms.get_mut(&vm_id) {
                state.status = VmStatus::Paused.into();
                info!("VM {} paused successfully (NOOP)", vm_id);
            } else {
                return Err(Status::not_found(format!("VM {} not found", vm_id)));
            }
        }

        Ok(Response::new(()))
    }

    async fn resume_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Resuming VM: {}", vm_id);

        // Update status
        {
            let mut vms = self.vms.lock().unwrap();
            if let Some(state) = vms.get_mut(&vm_id) {
                state.status = VmStatus::Running.into();
                info!("VM {} resumed successfully (NOOP)", vm_id);
            } else {
                return Err(Status::not_found(format!("VM {} not found", vm_id)));
            }
        }

        Ok(Response::new(()))
    }

    async fn delete_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        info!("Deleting VM: {}", vm_id);

        // Remove from memory
        {
            let mut vms = self.vms.lock().unwrap();
            if vms.remove(&vm_id).is_some() {
                info!("VM {} deleted successfully (NOOP)", vm_id);
            } else {
                return Err(Status::not_found(format!("VM {} not found", vm_id)));
            }
        }

        Ok(Response::new(()))
    }

    async fn get_vm_info(&self, request: Request<VmId>) -> Result<Response<VmState>, Status> {
        let vm_id = request.into_inner().id;
        info!("Getting VM info: {}", vm_id);

        let vms = self.vms.lock().unwrap();
        if let Some(state) = vms.get(&vm_id) {
            Ok(Response::new(state.clone()))
        } else {
            Err(Status::not_found(format!("VM {} not found", vm_id)))
        }
    }

    async fn list_vms(&self, _request: Request<()>) -> Result<Response<VmList>, Status> {
        info!("Listing VMs");

        let vms = self.vms.lock().unwrap();
        let vm_list: Vec<VmState> = vms.values().cloned().collect();

        info!("Found {} VMs", vm_list.len());
        Ok(Response::new(VmList { vms: vm_list }))
    }

    async fn add_network_device(
        &self,
        request: Request<AddNetworkDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Adding network device to VM: {}", req.vm_id);
        info!("Network device added successfully (NOOP)");
        Ok(Response::new(()))
    }

    async fn remove_network_device(
        &self,
        request: Request<RemoveDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Removing network device from VM: {}", req.vm_id);
        info!("Network device removed successfully (NOOP)");
        Ok(Response::new(()))
    }

    async fn add_disk_device(
        &self,
        request: Request<AddDiskDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Adding disk device to VM: {}", req.vm_id);
        info!("Disk device added successfully (NOOP)");
        Ok(Response::new(()))
    }

    async fn remove_disk_device(
        &self,
        request: Request<RemoveDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Removing disk device from VM: {}", req.vm_id);
        info!("Disk device removed successfully (NOOP)");
        Ok(Response::new(()))
    }
}
