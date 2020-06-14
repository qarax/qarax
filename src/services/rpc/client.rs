use tokio::runtime::{Builder, Runtime};

pub mod node {
    tonic::include_proto!("node");
}

use node::{node_client::NodeClient, Response as NodeResponse};

type StdError = Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T, E = StdError> = ::std::result::Result<T, E>;

pub struct Client {
    client: NodeClient<tonic::transport::Channel>,
    rt: Runtime,
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

        Ok(Self { rt, client })
    }

    pub fn health_check(
        &mut self,
        request: impl tonic::IntoRequest<()>,
    ) -> Result<tonic::Response<NodeResponse>, tonic::Status> {
        self.rt.block_on(self.client.health_check(request))
    }
}
