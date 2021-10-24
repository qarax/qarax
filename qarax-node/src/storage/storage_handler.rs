use tonic::{Request, Response, Status};

use crate::rpc::node::storage_service_server::StorageService;
use crate::rpc::node::{Response as NodeResponse, Storage};

#[derive(Debug, Default)]
pub(crate) struct StorageHandler {}

#[tonic::async_trait]
impl StorageService for StorageHandler {
    async fn create(&self, request: Request<Storage>) -> Result<Response<NodeResponse>, Status> {
        unimplemented!()
    }
}
