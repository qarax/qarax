use super::*;

use node::{node_client::NodeClient, Response as NodeResponse, VmConfig, VmId};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tonic::{IntoRequest, Request};
use tonic_health::proto::{health_client::HealthClient, HealthCheckRequest};

#[derive(Clone, Debug)]
pub struct Client {
    address: SocketAddr,
    client: Arc<RwLock<NodeClient<tonic::transport::Channel>>>,
}

impl Client {
    pub async fn connect(addr: SocketAddr) -> Result<Client, tonic::transport::Error> {
        let client = NodeClient::connect(format!("http://{}:{}", addr.ip(), addr.port())).await?;

        Ok(Self {
            address: addr,
            client: Arc::new(RwLock::new(client)),
        })
    }

    pub async fn health_check(&self) -> Result<(), tonic::Status> {
        let mut client = HealthClient::connect(format!(
            "http://{}:{}",
            self.address.ip(),
            self.address.port()
        ))
        .await
        .map_err(|e| tonic::Status::failed_precondition(e.to_string()))?;

        let request = tonic::Request::new(HealthCheckRequest {
            service: String::from("node.Node"),
        });

        client.check(request).await?;
        Ok(())
    }

    pub async fn start_vm(
        &self,
        request: impl tonic::IntoRequest<VmConfig>,
    ) -> Result<tonic::Response<VmConfig>, tonic::Status> {
        let response = self.client.write().await.start_vm(request).await?;
        Ok(response)
    }
}
