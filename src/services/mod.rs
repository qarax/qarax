pub mod host;
mod rpc;
mod util;

pub struct Backend {
    pub host_service: host::HostService,
}
