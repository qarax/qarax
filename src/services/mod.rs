pub mod host;
mod rpc;
mod util;
pub mod vm;

use anyhow::{anyhow, Context, Result};
use uuid::Uuid;

#[derive(Clone)]
pub struct Backend {
    pub host_service: host::HostService,
    pub vm_service: vm::VmService,
}
