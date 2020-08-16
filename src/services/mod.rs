use crate::database::DbConnection;
use anyhow::{anyhow, Context, Result};
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
    pub host_service: host::HostService,
    pub vm_service: vm::VmService,
    pub storage_service: storage::StorageService,
    pub drive_service: drive::DriveService,
    pub kernel_service: kernel::KernelService,
}
