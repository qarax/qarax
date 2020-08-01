use anyhow::{anyhow, Context, Result};
use uuid::Uuid;

pub mod host;
mod rpc;
mod util;
pub mod vm;

#[derive(Clone)]
pub struct Backend {
    pub host_service: host::HostService,
    pub vm_service: vm::VmService,
}
