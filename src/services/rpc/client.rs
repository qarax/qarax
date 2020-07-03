use std::sync::{Arc, RwLock};
use tokio::runtime::{Builder, Runtime};
use tokio::time::{self, Duration};

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
    async fn internal_connect<D>(
        dst: D,
    ) -> Result<NodeClient<tonic::transport::Channel>, tonic::Status>
    where
        D: std::convert::TryInto<tonic::transport::Endpoint> + Clone,
        D::Error: Into<StdError>,
    {
        let mut timeout = time::delay_for(Duration::from_secs(10));
        loop {
            tokio::select! {
               _ = &mut timeout => {
                   return Err(tonic::Status::deadline_exceeded("Could not connect in time"));
               },
               c = NodeClient::connect(dst.clone()) => match c {
                   Ok(c) => return Ok(c),

                   // TODO: the error needs to be looked at in case it's not "Connection refused"
                   Err(e) => continue,
               }
            };
        }
    }

    pub fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
    where
        D: std::convert::TryInto<tonic::transport::Endpoint> + Clone,
        D::Error: Into<StdError>,
    {
        let mut rt = Builder::new()
            .basic_scheduler()
            .enable_all()
            .build()
            .unwrap();

        let client = rt.block_on(Self::internal_connect(dst));

        Ok(Self {
            client: Arc::new(RwLock::new(client.unwrap())),
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
        request: impl tonic::IntoRequest<VmConfig>,
    ) -> Result<tonic::Response<VmResponse>, tonic::Status> {
        Arc::clone(&self.rt)
            .write()
            .unwrap()
            .block_on(self.client.write().unwrap().start_vm(request))
    }

    pub fn stop_vm(
        &self,
        request: impl tonic::IntoRequest<VmId>,
    ) -> Result<tonic::Response<NodeResponse>, tonic::Status> {
        Arc::clone(&self.rt)
            .write()
            .unwrap()
            .block_on(self.client.write().unwrap().stop_vm(request))
    }
}
