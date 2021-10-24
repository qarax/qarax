use super::*;

use std::fs;
use std::path::Path;

use tonic::{Request, Response, Status};

use crate::rpc::node::storage_service_server::StorageService;
use crate::rpc::node::{
    Response as NodeResponse, Status as NodeStatus, Storage, StorageType as RequestStorageType,
};

#[derive(Debug, Default)]
pub(crate) struct StorageHandler {}

#[tonic::async_trait]
impl StorageService for StorageHandler {
    async fn create(&self, request: Request<Storage>) -> Result<Response<NodeResponse>, Status> {
        let storage = request.into_inner();
        let path = Path::new(&STORAGE_PATH);
        let path = path.join(StorageType::from(storage.storage_type).to_string());
        let path = path.join(storage.storage_id);
        // TODO: create kernel_store and volume_store
        fs::create_dir_all(path)?;

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
