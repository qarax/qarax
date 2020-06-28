extern crate firecracker_rust_sdk;

use firecracker_rust_sdk::models::{boot_source, drive, machine, machine_configuration};
use node::{Status, VmConfig};

use std::convert::TryFrom;
use std::fmt;
use std::process::Command;

pub(crate) mod node {
    tonic::include_proto!("node");
}

type AsyncResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

// Make configurable
const FIRECRACKER_BIN: &str = "./firecracker";

// TODO: VmmHandler sounds like a stupid name
#[derive(Debug, Default)]
pub struct VmmHandler {
    machine: Option<Box<machine::Machine>>,
}

impl VmmHandler {
    pub fn new() -> Self {
        VmmHandler { machine: None }
    }

    pub async fn configure_vm(&mut self, vm_config: &VmConfig) {
        // TODO: do some actual validation
        let socket_path = format!("/tmp/{}.sock", vm_config.vm_id);
        let mc = machine_configuration::MachineConfiguration::new(false, vm_config.memory, 1);

        // TODO: boot_params should come from qarax and find a better way to handle kernel because it's already a string
        let bs = boot_source::BootSource::new(
            vm_config.kernel.to_string(),
            String::from("console=ttyS0 reboot=k panic=1 pci=off"),
        );

        // TODO: move into own function and handle errors
        // - check if socket exists before
        // - make a sanity check on the api server
        // - make it run in the background, not sure why it doesn't already
        let child = Command::new(FIRECRACKER_BIN)
            .args(vec!["--api-sock", &socket_path])
            .spawn()
            .expect("Faild to start firecracker");

        // TODO: proper polling that the api server is
        // available required here
        use std::{thread, time};
        thread::sleep(time::Duration::from_millis(1000));

        // TODO: get paths and ids from qarax
        let drive = drive::Drive::new(String::from("rootfs"), false, true, String::from("rootfs"));

        let vmm = machine::Machine::new(socket_path, mc, bs, drive, child.id());
        tokio::join!(vmm.configure_boot_source(), vmm.configure_drive());

        self.machine = Some(Box::new(vmm));
    }

    pub async fn start_vm(&self) {
        self.machine.as_ref().unwrap().start().await;
    }

    pub async fn stop_vm(&self) {
        self.machine.as_ref().unwrap().stop().await;
    }
}

impl TryFrom<i32> for Status {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Status::Success),
            1 => Ok(Status::Failure),
            _ => panic!("Shouldn't happen"),
        }
    }
}
