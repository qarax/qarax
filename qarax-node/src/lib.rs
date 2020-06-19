extern crate firecracker_rust_sdk;

use firecracker_rust_sdk::models::{machine_configuration, machine, boot_source};

pub async fn create_firecracker_client() {
    // TODO: do actual stuff
    let mc = machine_configuration::MachineConfiguration::new(false, 128, 1);
    let bs = boot_source::BootSource::new(String::from("vmlinux"));
    let machine = machine::Machine::new("/tmp/firecracker.sock", mc, bs);
    machine.start().await;
}
