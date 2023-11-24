use std::{path::PathBuf, sync::Arc};

use firec::MachineState;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::rpc::node::{vm_service_server::VmService, Drive, VmConfig, VmId, VmList, VmState};

use super::firecracker::{FirecrackerVmConfig, FirecrackerVmmManager};

#[derive(Debug)]
pub struct VmmService {
    firecracker_vmm_manager: Arc<RwLock<FirecrackerVmmManager<'static>>>,
}

impl VmmService {
    pub fn new() -> Self {
        Self {
            firecracker_vmm_manager: Arc::new(RwLock::new(FirecrackerVmmManager::new())),
        }
    }
}

#[tonic::async_trait]
impl VmService for VmmService {
    async fn start_vm(&self, request: Request<VmConfig>) -> Result<Response<VmConfig>, Status> {
        let vm_config = request.into_inner();
        let mut vmm_manager = self.firecracker_vmm_manager.write().await;
        match vmm_manager.start_vm(vm_config.clone().into()).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Failed to start VM: {}", e),
        }
        Ok(Response::new(vm_config))
    }
    async fn stop_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let inner = request.into_inner();
        let vm_id = Uuid::parse_str(inner.id.as_str())
            .map_err(|e| Status::invalid_argument(format!("Invalid VM ID: {}", e)))?;
        let mut vmm_manager = self.firecracker_vmm_manager.write().await;
        match vmm_manager.stop_vm(vm_id).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Failed to start VM: {}", e),
        }

        Ok(Response::new(()))
    }

    async fn list_vms(&self, _request: Request<()>) -> Result<Response<VmList>, Status> {
        Ok(Response::new(VmList { vms: vec![] }))
    }

    async fn get_vm_info(&self, request: Request<VmId>) -> Result<Response<VmState>, Status> {
        let inner = request.into_inner();
        let vm_id = Uuid::parse_str(inner.id.as_str())
            .map_err(|e| Status::invalid_argument(format!("Invalid VM ID: {}", e)))?;
        let vmm_manager = self.firecracker_vmm_manager.read().await;
        let firecracker_vm_config = vmm_manager
            .get_vm_info(vm_id)
            .map_err(|e| Status::internal(format!("Failed to get VM info: {}", e)))?;
        let vm_state = VmState {
            config: Some(VmConfig::from(firecracker_vm_config.0)),
            // TODO: move elsewhere, avoid having implementation details here
            status: match firecracker_vm_config.1 {
                MachineState::RUNNING => 1,
                MachineState::SHUTOFF => 2,
            },
        };

        Ok(Response::new(vm_state))
    }
}

impl From<VmConfig> for FirecrackerVmConfig {
    fn from(config: VmConfig) -> Self {
        let fc_drives = config
            .drives
            .into_iter()
            .map(|drive| (drive.id, PathBuf::from(drive.path_on_host), drive.is_root))
            .collect();

        Self {
            vm_id: config.vm_id,
            kernel: PathBuf::from(config.kernel),
            kernel_args: String::from(config.kernel_params),
            vcpus: config.vcpus as usize,
            memory: config.memory as i64,
            interfaces: vec![],
            drives: fc_drives,
            socket: PathBuf::from("./socket"),
            firecracker_exec: PathBuf::from("firecracker"),
        }
    }
}

impl From<FirecrackerVmConfig> for VmConfig {
    fn from(config: FirecrackerVmConfig) -> Self {
        let drives = config
            .drives
            .into_iter()
            .map(|(id, path, is_root)| {
                let drive = Drive {
                    id,
                    path_on_host: path.to_str().unwrap().to_string(),
                    is_root,
                    ..Default::default()
                };
                drive
            })
            .collect();

        Self {
            vm_id: config.vm_id,
            kernel: config.kernel.to_str().unwrap().to_string(),
            kernel_params: config.kernel_args,
            vcpus: config.vcpus as i32,
            memory: config.memory as i32,
            drives,
        }
    }
}
