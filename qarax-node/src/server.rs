use crate::server::node::node_server::Node;
use node::{Status as NodeStatus, Uuid, VmConfig, VmList, VmResponse};
use std::convert::TryFrom;
use tonic::{Request, Response, Status};

pub(crate) mod node {
    tonic::include_proto!("node");
}

impl TryFrom<i32> for NodeStatus {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(NodeStatus::Success),
            1 => Ok(NodeStatus::Failure),
            _ => panic!("Shouldn't happen"),
        }
    }
}

#[derive(Debug, Default)]
pub struct QaraxNode {}

#[tonic::async_trait]
impl Node for QaraxNode {
    async fn start_vm(&self, request: Request<VmConfig>) -> Result<Response<VmResponse>, Status> {
        println!("Start VM: {:?}", request);
        let response = VmResponse {
            status: NodeStatus::Success as i32,
            config: Some(VmConfig {
                vm_id: Some(Uuid {
                    value: String::from("123"),
                }),
                vcpus: 1,
                memory: 128,
                kernel: String::from("vmlinux"),
                root_fs: String::from("rootfs"),
            }),
        };

        Ok(Response::new(response))
    }

    async fn stop_vm(&self, request: Request<Uuid>) -> Result<Response<node::Response>, Status> {
        println!("Got a request: {:?}", request);

        let response = node::Response {
            status: NodeStatus::Success as i32,
        };

        Ok(Response::new(response))
    }

    async fn list_vms(&self, request: Request<()>) -> Result<Response<node::VmList>, Status> {
        println!("Got a request: {:?}", request);

        let response = VmList {
            vm_id: vec![Uuid {
                value: String::from("123"),
            }],
        };

        Ok(Response::new(response))
    }

    async fn health_check(&self, request: Request<()>) -> Result<Response<node::Response>, Status> {
        println!("Got a request: {:?}", request);

        let response = node::Response {
            status: NodeStatus::Success as i32,
        };

        Ok(Response::new(response))
    }
}
