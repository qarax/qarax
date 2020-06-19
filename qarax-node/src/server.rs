use crate::lib;
use crate::lib::node::node_server::Node;
use crate::lib::node::{
    Response as NodeResponse, Status as NodeStatus, Uuid, VmConfig, VmList, VmResponse,
};

use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
pub struct QaraxNode {}

#[tonic::async_trait]
impl Node for QaraxNode {
    async fn start_vm(&self, request: Request<VmConfig>) -> Result<Response<VmResponse>, Status> {
        println!("Start VM: {:?}", request);
        let config = VmConfig {
            vm_id: Some(Uuid {
                value: String::from("123"),
            }),
            vcpus: 1,
            memory: 128,
            kernel: String::from("./vmlinux"),
            root_fs: String::from("rootfs"),
        };

        lib::start_vm(&config).await;

        let response = VmResponse {
            status: NodeStatus::Success as i32,
            config: Some(config),
        };

        // TODO: there is no reason to return the config now
        Ok(Response::new(response))
    }

    async fn stop_vm(&self, request: Request<Uuid>) -> Result<Response<NodeResponse>, Status> {
        println!("Got a request: {:?}", request);

        let response = NodeResponse {
            status: NodeStatus::Success as i32,
        };

        Ok(Response::new(response))
    }

    async fn list_vms(&self, request: Request<()>) -> Result<Response<VmList>, Status> {
        println!("Got a request: {:?}", request);

        let response = VmList {
            vm_id: vec![Uuid {
                value: String::from("123"),
            }],
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
