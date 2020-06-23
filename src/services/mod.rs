pub mod host;
pub mod vm;
mod rpc;
mod util;

pub struct Backend {
    pub host_service: host::HostService,
    pub vm_service: vm::VmService,
}
