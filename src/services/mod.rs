use crate::database::DbConnection;
use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use uuid::Uuid;

pub mod drive;
pub mod host;
pub mod kernel;
mod rpc;
pub mod storage;
mod util;
pub mod vm;

#[derive(Clone)]
pub struct Backend {
    pub host_service: Arc<host::HostService>,
    pub vm_service: Arc<vm::VmService>,
    pub storage_service: Arc<storage::StorageService>,
    pub drive_service: Arc<drive::DriveService>,
    pub kernel_service: Arc<kernel::KernelService>,
}
