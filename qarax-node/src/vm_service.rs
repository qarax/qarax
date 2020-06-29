use crate::vmm_handler::node::node_server::Node;
use crate::vmm_handler::node::{
    Response as NodeResponse, Status as NodeStatus, VmConfig, VmId, VmList, VmResponse,
};
use crate::vmm_handler::VmmHandler;

use std::collections::HashMap;

use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Code, Request, Response, Status};

#[derive(Debug)]
pub struct VmService {
    // TODO: Not sure about this at all
    handlers: Arc<RwLock<HashMap<String, VmmHandler>>>,
    log: slog::Logger,
}

impl VmService {
    pub fn new(log: slog::Logger) -> Self {
        VmService {
            handlers: Arc::new(RwLock::new(HashMap::new())),
            log,
        }
    }
}

#[tonic::async_trait]
impl Node for VmService {
    async fn start_vm(&self, request: Request<VmConfig>) -> Result<Response<VmResponse>, Status> {
        let config = request.into_inner();
        info!(self.log, "Starting VM {}", config.vm_id; "vm_id" => &config.vm_id, "vcpus" => &config.vcpus);

        let mut handlers = self.handlers.write().await;
        let handler = handlers
            .entry(config.vm_id.to_owned())
            .or_insert(VmmHandler::new(self.log.new(o!())));
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
        let vm_id = request.into_inner().vm_id;
        info!(self.log, "Stopping VM {}", vm_id; "vm_id" => &vm_id);
        info!(
            self.log,
            "{} handlers available",
            self.handlers.read().await.len()
        );

        let mut handlers = self.handlers.write().await;

        if let Some(handler) = handlers.get(&vm_id) {
            info!(self.log, "Fetched handler for vm {}", vm_id; "vm_id" => &vm_id);
            handler.stop_vm().await;

            handlers.remove(&vm_id);
            let response = NodeResponse {
                status: NodeStatus::Success as i32,
            };

            Ok(Response::new(response))
        } else {
            return Err(Status::new(Code::FailedPrecondition, "vm not found"));
        }
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
