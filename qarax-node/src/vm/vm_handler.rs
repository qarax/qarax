use std::{process::Stdio, sync::Arc};

use super::node::VmConfig;
use anyhow::{anyhow, Result};
use firecracker_rust_sdk::models::{boot_source, drive, logger, machine, machine_configuration};
use tokio::{fs::OpenOptions, io::AsyncWriteExt, process::Command, sync::RwLock};

// Make configurable
const FIRECRACKER_BIN: &str = "./firecracker";

#[derive(Debug, Default)]
pub struct VmHandler {
    machine: Arc<RwLock<Option<machine::Machine>>>,
}

impl VmHandler {
    pub async fn configure_vm(&mut self, vm_config: &mut VmConfig) -> Result<()> {
        tracing::info!("Configuring VMM...");

        // TODO: do some actual validation
        let socket_path = format!("/tmp/{}.sock", vm_config.vm_id);
        let mc = machine_configuration::MachineConfiguration::new(
            false,
            vm_config.memory,
            vm_config.vcpus,
        );

        // TODO: boot_params should come from qarax and find a better way to handle kernel because it's already a string
        let bs = boot_source::BootSource::new(
            Some(vm_config.kernel_params.to_string()),
            vm_config.kernel.to_string(),
        );

        // TODO: implement From
        let fc_drives = vm_config
            .drives
            .iter()
            .map(|drive| drive::Drive {
                drive_id: drive.drive_id.clone(),
                cache_type: None,
                is_read_only: drive.is_read_only,
                is_root_device: drive.is_root_device,
                path_on_host: drive.path_on_host.clone(),
                partuuid: None,
                rate_limiter: None,
            })
            .collect();

        let mut logger = logger::Logger::new(format!("/var/log/{}.log", vm_config.vm_id));
        // TODO: get the level from qarax-node's configuration (hopefully it'll have one)
        logger.level = Some(logger::Level::Debug);
        create_log_pipe(&logger)?;

        let vmm = machine::Machine::new(
            vm_config.vm_id.to_owned(),
            socket_path,
            mc,
            bs,
            fc_drives,
            Vec::new(),
            logger,
            None,
        );

        self.machine.write().await.replace(vmm);
        Ok(())
    }

    pub async fn start_vm(&self) -> Result<()> {
        let mut machine_handler = self.machine.write().await;

        if machine_handler.is_none() {
            Err(anyhow!("No machine object!"))
        } else {
            let machine = machine_handler.as_mut().unwrap();
            let socket_path = &format!("/tmp/{}.sock", machine.vm_id);
            let config_file = self.create_config_file(machine).await?;
            let args = vec!["--api-sock", socket_path, "--config-file", &config_file];
            tracing::info!("Starting firecracker with args: '{:#?}'", args);

            let child = Command::new(FIRECRACKER_BIN)
                .args(args)
                .stdout(Stdio::null())
                .spawn()
                .expect("Faild to start firecracker");

            machine.set_pid(child.id().unwrap());

            Ok(())
        }
    }

    async fn create_config_file(&self, machine: &machine::Machine) -> Result<String> {
        let path = format!("/var/run/{}.config", machine.vm_id);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await?;

        let config = serde_json::to_string(&machine).unwrap();
        tracing::info!("Writing config {}", config);

        file.write_all(config.as_bytes()).await?;
        Ok(path)
    }
}

fn create_log_pipe(logger: &logger::Logger) -> Result<()> {
    use nix::{sys::stat, unistd};
    use std::path::Path;

    unistd::mkfifo(Path::new(&logger.log_path), stat::Mode::S_IRWXU)?;
    Ok(())
}
