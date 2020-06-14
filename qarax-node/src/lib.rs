extern crate firecracker_rust_sdk;

use firecracker_rust_sdk::models;

fn create_firecracker_client() {
    // TODO: do actual stuff
    models::MachineConfiguration::new(true, 123, 1);
}
