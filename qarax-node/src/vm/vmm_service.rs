extern crate firecracker_rust_sdk;

use crate::rpc::node::vm_service_server::VmService;
use crate::rpc::node::{Response as NodeResponse, VmConfig, VmId, VmList};
use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
pub struct VmmService {}

#[tonic::async_trait]
impl VmService for VmmService {
    async fn start_vm(&self, request: Request<VmConfig>) -> Result<Response<VmConfig>, Status> {
        unimplemented!()
    }

    async fn stop_vm(&self, _request: Request<VmId>) -> Result<Response<NodeResponse>, Status> {
        unimplemented!()
    }

    async fn list_vms(&self, _request: Request<()>) -> Result<Response<VmList>, Status> {
        unimplemented!()
    }
}
