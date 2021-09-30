extern crate firecracker_rust_sdk;

use super::node::node_server::Node;
use super::node::{Response as NodeResponse, Status as NodeStatus, VmConfig, VmId, VmList};
use super::vm_handler::VmHandler;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Code, Request, Response, Status};

#[derive(Debug, Default)]
pub struct VmmService {
    handlers: Arc<RwLock<HashMap<String, VmHandler>>>,
}

#[tonic::async_trait]
impl Node for VmmService {
    async fn start_vm(&self, request: Request<VmConfig>) -> Result<Response<VmConfig>, Status> {
        let mut config = request.into_inner();
        tracing::info!(
            "Starting VM {}, {}, {}",
            &config.vm_id,
            &config.memory,
            &config.vcpus
        );

        let mut handlers = self.handlers.write().await;
        let handler = handlers
            .entry(config.vm_id.to_owned())
            .or_insert_with(VmHandler::default);

        handler
            .configure_vm(&mut config)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        tracing::info!("Configured VM...");
        handler
            .start_vm()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        tracing::info!("Started VM...");

        Ok(Response::new(config))
    }

    async fn stop_vm(&self, request: Request<VmId>) -> Result<Response<NodeResponse>, Status> {
        unimplemented!()
    }

    async fn list_vms(&self, request: Request<()>) -> Result<Response<VmList>, Status> {
        unimplemented!()
    }
}
