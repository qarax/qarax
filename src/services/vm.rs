use super::*;
use crate::database::DbConnection;
use crate::models::storage::StorageType;
use crate::models::vm::{NetworkMode, NewVm, Vm};

use crate::services::drive::{DriveService, LocalVolume, Volume};
use crate::services::host::HostService;
use crate::services::kernel::KernelService;

use std::str::FromStr;

#[derive(Clone)]
pub struct VmService {
    host_service: Arc<HostService>,
    drive_service: Arc<DriveService>,
    kernel_service: Arc<KernelService>,
}

impl VmService {
    pub fn new(
        host_service: Arc<HostService>,
        drive_service: Arc<DriveService>,
        kernel_service: Arc<KernelService>,
    ) -> Self {
        VmService {
            host_service: Arc::clone(&host_service),
            drive_service: Arc::clone(&drive_service),
            kernel_service: Arc::clone(&kernel_service),
        }
    }

    pub fn get_by_id(&self, vm_id: &str, conn: &DbConnection) -> Result<Vm> {
        Vm::by_id(Uuid::parse_str(vm_id).unwrap(), conn)
    }

    pub fn get_all(&self, conn: &DbConnection) -> Result<Vec<Vm>> {
        Vm::all(conn)
    }

    pub fn add_vm(&self, vm: &NewVm, conn: &DbConnection) -> Result<Uuid> {
        Vm::insert(vm, conn)
    }

    pub fn start(&self, vm_id: &str, conn: &DbConnection) -> Result<Uuid> {
        use super::rpc::client::node::VmConfig;

        let host = self.host_service.get_running_host(conn)?;
        let client = self.host_service.get_client(host.id)?;
        let mut vm = self.get_by_id(vm_id, conn).unwrap();
        let clone = vm.clone();
        let request = VmConfig {
            vm_id: clone.id.to_string(),
            memory: clone.memory,
            vcpus: clone.vcpu,
            kernel: self.get_kernel_path(&vm.kernel.to_string(), conn)?,
            kernel_params: clone.kernel_params,
            network_mode: clone.network_mode.clone(),
            ip_address: clone.ip_address.unwrap_or_else(String::new),
            mac_address: clone.mac_address.unwrap_or_else(String::new),
            drives: self.create_vm_drives(&vm, conn)?,
        };

        println!("sending vm config {:#?}", request);

        match client.start_vm(request) {
            Ok(config) => {
                if NetworkMode::from_str(&vm.network_mode)? == NetworkMode::Dhcp {
                    let inner: &VmConfig = &config.into_inner();
                    vm.ip_address = Some(inner.ip_address.clone());
                    vm.mac_address = Some(inner.mac_address.clone());
                    vm.host_id = Some(host.id);
                    Vm::update(&vm, conn)?;
                }
            }
            Err(e) => {
                return Err(e.into());
            }
        }

        Ok(vm.id)
    }

    pub fn stop(&self, vm_id: &str, conn: &DbConnection) -> Result<Uuid> {
        use super::rpc::client::node::VmId;

        let host = self.host_service.get_running_host(conn)?;
        let client = self.host_service.get_client(host.id)?;
        let request = VmId {
            vm_id: vm_id.to_owned(),
        };
        client.stop_vm(request)?;

        Ok(Uuid::parse_str(vm_id).unwrap())
    }

    pub fn attach_drive(&self, vm_id: String, drive_id: String, conn: &DbConnection) -> Result<()> {
        Vm::attach_drive(Uuid::parse_str(&vm_id)?, Uuid::parse_str(&drive_id)?, conn)
    }

    fn create_vm_drives(
        &self,
        vm: &Vm,
        conn: &DbConnection,
    ) -> Result<Vec<super::rpc::client::node::Drive>> {
        use super::rpc::client::node::Drive as VmDrive;

        let drives = self.drive_service.get_drives_for_vms(vm, conn)?;
        Ok(drives
            .iter()
            .map(|drive| {
                let storage = self
                    .drive_service
                    .get_storage(drive.id.to_string(), conn)
                    .unwrap();
                let drive_id = drive.id.to_string();

                // TODO: add some Volume::new() type of method
                let volume: Box<dyn Volume>;
                if storage.storage_type == StorageType::Local {
                    volume = Box::new(LocalVolume::new(drive_id.as_str(), &storage.config))
                } else {
                    // TODO: implement shared
                    unimplemented!()
                }

                VmDrive {
                    drive_id: drive.id.to_string(),
                    is_read_only: drive.readonly,
                    is_root_device: drive.rootfs,
                    path_on_host: volume.get_path(),
                }
            })
            .collect())
    }

    fn get_kernel_path(&self, kernel_id: &str, conn: &DbConnection) -> Result<String> {
        let storage = self.kernel_service.get_storage(kernel_id, conn)?;

        // TODO: add some Volume::new() type of method
        let volume: Box<dyn Volume>;
        if storage.storage_type == StorageType::Local {
            volume = Box::new(LocalVolume::new(kernel_id, &storage.config));
        } else {
            // TODO: implement shared
            unimplemented!()
        }

        Ok(volume.get_path())
    }

    #[allow(dead_code)]
    pub fn delete_all(&self, conn: &DbConnection) -> Result<usize, String> {
        match Vm::delete_all(conn) {
            Ok(record_count) => Ok(record_count),
            Err(e) => Err(e.to_string()),
        }
    }
}
