use crate::vmm_handler::node::node_server::Node;
use crate::vmm_handler::node::{
    Response as NodeResponse, Status as NodeStatus, VmConfig, VmId, VmList, VmResponse,
};
use crate::vmm_handler::VmmHandler;

use std::collections::HashMap;

use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
pub struct VmService {
    // TODO: Not sure about this at all
    handlers: Arc<RwLock<HashMap<String, VmmHandler>>>,
}

impl VmService {
    pub fn new() -> Self {
        VmService {
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[tonic::async_trait]
impl Node for VmService {
    async fn start_vm(&self, request: Request<VmConfig>) -> Result<Response<VmResponse>, Status> {
        println!("Start VM: {:?}", request);
        let config = request.into_inner();
        let mut handlers = self.handlers.write().await;
        let handler = handlers
            .entry(config.vm_id.to_owned())
            .or_insert(VmmHandler::new());
        handler.configure_vm(&config).await;
        handler.start_vm().await;

        let response = VmResponse {
            status: NodeStatus::Success as i32,
            config: Some(config),
        };

        // TODO: there is no reason to return the config now
        Ok(Response::new(response))
    }

    async fn stop_vm(&self, request: Request<VmId>) -> Result<Response<NodeResponse>, Status> {
        println!("Got a request: {:?}", request);
        let vm_id = request.into_inner().vm_id;
        let mut handlers = self.handlers.write().await;
        let handler = handlers.entry(vm_id.clone()).or_insert(VmmHandler::new());
        handler.stop_vm().await;

        handlers.remove(&vm_id);
        let response = NodeResponse {
            status: NodeStatus::Success as i32,
        };

        Ok(Response::new(response))
    }

    async fn list_vms(&self, request: Request<()>) -> Result<Response<VmList>, Status> {
        println!("Got a request: {:?}", request);

        let response = VmList {
            vm_id: vec![String::from("123")],
        };

        Ok(Response::new(response))
    }

    async fn health_check(&self, request: Request<()>) -> Result<Response<NodeResponse>, Status> {
        println!("Got a request: {:?}", request);

        let response = NodeResponse {
            status: NodeStatus::Success as i32,
        };

        Ok(Response::new(response))
    }
}
