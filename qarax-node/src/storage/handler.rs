use super::*;

use std::path::Path;
use tokio::fs::{self, File};

use tonic::{Request, Response, Status};
use tracing::instrument;

use crate::rpc::node::storage_service_server::StorageService;
use crate::rpc::node::{
    Response as NodeResponse, Status as NodeStatus, Storage, VolumeRequest, VolumeType,
};

#[derive(Debug, Default)]
pub(crate) struct StorageHandler {}

#[tonic::async_trait]
impl StorageService for StorageHandler {
    #[instrument]
    async fn create(&self, request: Request<Storage>) -> Result<Response<NodeResponse>, Status> {
        tracing::info!("request metadata {:?}", request.metadata());
        request
            .metadata()
            .get("request_id")
            .map(|id| tracing::info!("request_id: {:?}", id.to_str()));

        let storage = request.into_inner();

        let path = Path::new(&STORAGE_PATH);
        let path = path.join(storage.storage_id);

        let storage_type = StorageType::from(storage.storage_type);
        if storage_type == StorageType::Local {
            let path_on_host = storage.config.unwrap().path_on_host.unwrap();
            tracing::info!("Creating symlink from {:?} to {:?}", path, path_on_host);
            tokio::fs::symlink(path_on_host, &path).await?;
        }

        // TODO: create kernel_store and volume_store
        fs::create_dir_all(path.join("kernels")).await?;
        fs::create_dir_all(path.join("drives")).await?;

        let response = NodeResponse {
            status: NodeStatus::Success as i32,
        };

        Ok(Response::new(response))
    }

    #[instrument]
    async fn create_volume(
        &self,
        request: Request<VolumeRequest>,
    ) -> Result<Response<NodeResponse>, Status> {
        tracing::info!("request metadata {:?}", request.metadata());
        request
            .metadata()
            .get("request_id")
            .map(|id| tracing::info!("request_id: {:?}", id.to_str()));

        // Create empty file
        let volume: VolumeRequest = request.into_inner();

        let path = Path::new(&STORAGE_PATH);
        let path = path.join(volume.storage.unwrap().storage_id);

        let volume_type = VolumeType::from(volume.volume_type);
        let volume_path = match volume_type {
            VolumeType::Drive => "drives",
            VolumeType::Kernel => "kernels",
        };

        let path = path.join(volume_path);
        let path = path.join(volume.volume_id);
        let file = File::create(&path).await?;
        file.set_len(volume.size as u64).await?;
        tracing::info!(file = ?path.as_os_str(), size = volume.size, "Created volume");

        if let Some(url) = volume.url {
            let size = volume.size;
            tracing::info!(url = ?url.clone(), "Downloading from url");
            download::download(&url, &path, size as u64).await.unwrap();
        }

        let response = NodeResponse {
            status: NodeStatus::Success as i32,
        };

        Ok(Response::new(response))
    }
}

impl From<i32> for StorageType {
    fn from(st: i32) -> Self {
        match st {
            0 => StorageType::Local,
            1 => StorageType::Shared,
            _ => panic!("Unknown storage type"),
        }
    }
}

impl From<i32> for VolumeType {
    fn from(st: i32) -> Self {
        match st {
            0 => VolumeType::Drive,
            1 => VolumeType::Kernel,
            _ => panic!("Unknown storage type"),
        }
    }
}
