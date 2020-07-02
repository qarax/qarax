use std::sync::{Arc, RwLock};
use tokio::runtime::{Builder, Runtime};

pub mod node {
    tonic::include_proto!("node");
}

use node::{node_client::NodeClient, Response as NodeResponse, VmConfig, VmId, VmResponse};

type StdError = Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T, E = StdError> = ::std::result::Result<T, E>;

#[derive(Clone)]
pub struct Client {
    rt: Arc<RwLock<Runtime>>,
    client: Arc<RwLock<NodeClient<tonic::transport::Channel>>>,
}

impl Client {
    pub fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
    where
        D: std::convert::TryInto<tonic::transport::Endpoint>,
        D::Error: Into<StdError>,
    {
        let mut rt = Builder::new()
            .basic_scheduler()
            .enable_all()
            .build()
            .unwrap();
        let client = rt.block_on(NodeClient::connect(dst))?;

        Ok(Self {
            client: Arc::new(RwLock::new(client)),
            rt: Arc::new(RwLock::new(rt)),
        })
    }

    pub fn health_check(
        &self,
        request: impl tonic::IntoRequest<()>,
    ) -> Result<tonic::Response<NodeResponse>, tonic::Status> {
        Arc::clone(&self.rt)
            .write()
            .unwrap()
            .block_on(self.client.write().unwrap().health_check(request))
    }

    pub fn start_vm(
        &self,
        request: impl tonic::IntoRequest<(VmConfig)>,
    ) -> Result<tonic::Response<VmResponse>, tonic::Status> {
        Arc::clone(&self.rt)
            .write()
            .unwrap()
            .block_on(self.client.write().unwrap().start_vm(request))
    }

    pub fn stop_vm(
        &self,
        request: impl tonic::IntoRequest<(VmId)>,
    ) -> Result<tonic::Response<NodeResponse>, tonic::Status> {
        Arc::clone(&self.rt)
            .write()
            .unwrap()
            .block_on(self.client.write().unwrap().stop_vm(request))
    }
}
