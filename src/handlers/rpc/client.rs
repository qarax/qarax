use super::*;

use crate::handlers::models::storage::{Storage, StorageType};

use node::storage_service_client::StorageServiceClient;
use node::{
    vm_service_client::VmServiceClient, Response as NodeResponse, Storage as StorageConfig,
    VmConfig,
};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;

use tonic_health::proto::{health_client::HealthClient, HealthCheckRequest};

#[derive(Clone, Debug)]
pub struct VmmClient {
    address: SocketAddr,
    client: Arc<RwLock<VmServiceClient<tonic::transport::Channel>>>,
}

impl VmmClient {
    pub async fn connect(addr: SocketAddr) -> Result<Self, tonic::transport::Error> {
        let client =
            VmServiceClient::connect(format!("http://{}:{}", addr.ip(), addr.port())).await?;

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
            service: String::from("node.VmService"),
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

#[derive(Clone, Debug)]
pub struct StorageClient {
    address: SocketAddr,
    client: Arc<RwLock<StorageServiceClient<tonic::transport::Channel>>>,
}

impl StorageClient {
    pub async fn connect(addr: SocketAddr) -> Result<Self, tonic::transport::Error> {
        let client =
            StorageServiceClient::connect(format!("http://{}:{}", addr.ip(), addr.port())).await?;

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
            service: String::from("node.StorageService"),
        });

        client.check(request).await?;
        Ok(())
    }

    pub async fn create(
        &self,
        storage: Storage,
    ) -> Result<tonic::Response<NodeResponse>, tonic::Status> {
        self.client
            .write()
            .await
            .create(StorageConfig::from(storage))
            .await
    }
}

impl From<Storage> for StorageConfig {
    fn from(storage: Storage) -> Self {
        Self {
            storage_id: storage.id.to_string(),
            storage_name: storage.name,
            storage_type: match storage.storage_type {
                StorageType::Local => 0,
                StorageType::Shared => 1,
            },
        }
    }
}
