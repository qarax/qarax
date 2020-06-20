use crate::vmm_handler::VmmHandler;
use crate::vmm_handler::node::node_server::Node;
use crate::vmm_handler::node::{
    Response as NodeResponse, Status as NodeStatus, VmConfig, VmList, VmResponse, VmId
};

use std::collections::HashMap;

use tonic::{Request, Response, Status};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct VmService {
    // TODO: Not sure about this at all
    handlers: RwLock<HashMap<String, VmmHandler>>
}

impl VmService {
    pub fn new() -> Self {
       VmService {
           handlers: RwLock::new(HashMap::new()),
       }
    }
}

#[tonic::async_trait]
impl Node for VmService {
    async fn start_vm(&self, request: Request<VmConfig>) -> Result<Response<VmResponse>, Status> {
        println!("Start VM: {:?}", request);
        let config = VmConfig {
            vm_id:  String::from("123"),
            vcpus: 1,
            memory: 128,
            kernel: String::from("./vmlinux"),
            root_fs: String::from("rootfs"),
        };

        let mut handlers = self.handlers.write().await;
        let handler = handlers.entry(config.vm_id.to_owned()).or_insert(VmmHandler::new());
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
