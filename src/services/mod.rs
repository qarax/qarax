pub mod host;
mod rpc;
mod util;
pub mod vm;

pub struct Backend {
    pub host_service: host::HostService,
    pub vm_service: vm::VmService,
}
