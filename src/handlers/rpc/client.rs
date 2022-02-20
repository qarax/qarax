use super::*;

use crate::models::storage::{Storage, StorageType};

use node::storage_service_client::StorageServiceClient;
use node::{
    vm_service_client::VmServiceClient, Response as NodeResponse, Storage as StorageConfig,
    VmConfig,
};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::{Request, Status};
use tracing::instrument;

use tonic_health::proto::{health_client::HealthClient, HealthCheckRequest};

#[derive(Clone, Debug)]
pub struct VmmClient {
    address: SocketAddr,
    client: Arc<RwLock<VmServiceClient<Channel>>>,
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
    channel: Channel,
}

impl StorageClient {
    pub async fn connect(addr: SocketAddr) -> Result<Self, tonic::transport::Error> {
        let address = format!("http://{}:{}", addr.ip(), addr.port());
        let channel = Channel::from_shared(address).unwrap().connect_lazy();
        Ok(Self {
            address: addr,
            channel,
        })
    }

    #[instrument]
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

    #[instrument]
    pub async fn create(
        &self,
        request: StorageCreateRequest,
    ) -> Result<tonic::Response<NodeResponse>, tonic::Status> {
        let interceptor = RequestIdInterceptor {
            request_id: Some(request.request_id),
        };

        let channel = self.channel.clone();
        let mut client = StorageServiceClient::with_interceptor(channel, interceptor);
        client.create(StorageConfig::from(request.storage)).await
    }
}

struct RequestIdInterceptor {
    request_id: Option<String>,
}

impl Interceptor for RequestIdInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if let Some(request_id) = &self.request_id {
            request
                .metadata_mut()
                .insert("request_id", MetadataValue::from_str(request_id).unwrap());
        }

        Ok(request)
    }
}

pub struct StorageCreateRequest {
    pub storage: Storage,
    pub request_id: String,
}

impl std::fmt::Debug for StorageCreateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageCreateRequest")
            .field("storage", &self.storage)
            .field("request_id", &self.request_id)
            .finish()
    }
}

impl From<StorageCreateRequest> for Request<StorageCreateRequest> {
    fn from(r: StorageCreateRequest) -> Self {
        Request::new(StorageCreateRequest {
            storage: r.storage,
            request_id: r.request_id,
        })
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
