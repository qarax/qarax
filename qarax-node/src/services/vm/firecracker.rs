use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use firec::{
    config::{network::Interface, Config, JailerMode},
    Machine, MachineState,
};
use tracing::{error, info};
use uuid::Uuid;

const CHROOT_BASE: &str = "/var/tmp/fc";

#[derive(Debug)]
pub struct FirecrackerVmConfig {
    pub vm_id: String,
    pub kernel: PathBuf,
    pub kernel_args: String,
    pub vcpus: usize,
    pub memory: i64,
    pub interfaces: Vec<FirecrackerInterface>,
    pub drives: Vec<(String, PathBuf, bool)>,
    pub socket: PathBuf,
    pub firecracker_exec: PathBuf,
}

#[derive(Debug)]
pub struct FirecrackerVmmManager<'a> {
    pub vms: HashMap<Uuid, Machine<'a>>,
}

impl<'a> FirecrackerVmmManager<'a> {
    pub fn new() -> Self {
        Self {
            vms: HashMap::new(),
        }
    }

    #[tracing::instrument]
    pub async fn start_vm(&mut self, input: FirecrackerVmConfig) -> anyhow::Result<()> {
        let vm_id = Uuid::parse_str(&input.vm_id)?;
        let mut config = Config::builder(Some(vm_id), input.kernel.clone())
            .jailer_cfg()
            .chroot_base_dir(Path::new(CHROOT_BASE))
            .cgroup_version(firec::config::CgroupVersion::V2)
            .exec_file(input.firecracker_exec.clone())
            .build()
            .kernel_args(input.kernel_args)
            .machine_cfg()
            .vcpu_count(input.vcpus)
            .mem_size_mib(input.memory)
            .build();
        // TODO: add network interface
        for drive in input.drives {
            config = config
                .add_drive(drive.0, drive.1)
                .is_root_device(drive.2)
                .build();
        }
        let config = config.socket_path(input.socket).build();
        let mut machine = Machine::create(config).await?;

        info!("Starting VM {}", vm_id);
        machine.start().await?;
        self.vms.insert(vm_id, machine);
        Ok(())
    }

    #[tracing::instrument]
    pub async fn stop_vm(&mut self, vm_id: Uuid) -> anyhow::Result<()> {
        if let Some(machine) = self.vms.get_mut(&vm_id) {
            machine.force_shutdown().await?;
        } else {
            error!("VM {} not found", vm_id)
        }

        Ok(())
    }

    #[tracing::instrument]
    pub fn get_vm_info(&self, vm_id: Uuid) -> anyhow::Result<(FirecrackerVmConfig, MachineState)> {
        if let Some(machine) = self.vms.get(&vm_id) {
            Ok((FirecrackerVmConfig::from(machine), machine.state()))
        } else {
            Err(anyhow::anyhow!("VM {} not found", vm_id))
        }
    }
}

impl<'a> From<&Machine<'a>> for FirecrackerVmConfig {
    fn from(machine: &Machine<'a>) -> Self {
        let config = machine.config();

        let drives = config
            .drives()
            .iter()
            .map(|drive| {
                (
                    drive.drive_id().to_string(),
                    drive.src_path().to_path_buf(),
                    drive.is_root_device(),
                )
            })
            .collect();

        let machine_cfg = config.machine_cfg();
        let kernel_args = config.kernel_args().clone().unwrap_or_else(|| "".into());

        Self {
            vm_id: config.vm_id().to_string(),
            kernel: config.kernel_image_path().to_path_buf(),
            kernel_args: kernel_args.to_string(),
            vcpus: machine_cfg.vcpu_count(),
            memory: machine_cfg.mem_size_mib(),
            interfaces: config
                .network_interfaces()
                .iter()
                .map(FirecrackerInterface::from)
                .collect(),
            drives,
            socket: config.socket_path().to_path_buf(),
            firecracker_exec: PathBuf::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FirecrackerInterface {
    host_if_name: String,
    vm_if_name: String,
}

impl FirecrackerInterface {
    fn from(interface: &Interface) -> Self {
        Self {
            host_if_name: interface.host_if_name().to_string(),
            vm_if_name: interface.vm_if_name().to_string(),
        }
    }
}
