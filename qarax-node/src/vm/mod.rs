pub(crate) mod node {
    tonic::include_proto!("node");
}

mod vm_handler;
pub mod vmm_service;
