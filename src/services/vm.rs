use super::*;
use crate::database::DbConnection;
use crate::models::vm::{NetworkMode, NewVm, Vm};
use crate::services::host::HostService;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Copy, Clone)]
pub struct VmService {}

impl VmService {
    pub fn new() -> Self {
        VmService {}
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

    pub fn start(
        &self,
        vm_id: &str,
        host_service: &HostService,
        conn: &DbConnection,
    ) -> Result<Uuid> {
        use super::rpc::client::node::VmConfig;

        // TODO: error handling
        let host = host_service.get_running_host(conn);
        let client = host_service.get_client(host.id);
        let mut vm = self.get_by_id(vm_id, conn).unwrap();
        let clone = vm.clone();
        let request = VmConfig {
            vm_id: clone.id.to_string(),
            memory: clone.memory,
            vcpus: clone.vcpu,
            kernel: clone.kernel,
            root_fs: clone.root_file_system,
            kernel_params: clone.kernel_params,
            network_mode: clone.network_mode.clone().unwrap_or_else(String::new),
            address: clone.address.unwrap_or_else(String::new),
        };

        match client.start_vm(request) {
            Ok(config) => {
                let network_mode = vm.network_mode.as_ref().unwrap();
                if NetworkMode::from_str(network_mode.as_str()).unwrap() == NetworkMode::Dhcp {
                    vm.address = Some(config.into_inner().address);
                    Vm::update(&vm, conn)?;
                }
            }
            Err(e) => {
                return Err(e.into());
            }
        }

        Ok(vm.id)
    }

    pub fn stop(
        &self,
        vm_id: &str,
        host_service: &HostService,
        conn: &DbConnection,
    ) -> Result<Uuid> {
        use super::rpc::client::node::VmId;

        // TODO: error handling
        let host = host_service.get_running_host(conn);
        let client = host_service.get_client(host.id);
        let request = VmId {
            vm_id: vm_id.to_owned(),
        };
        client.stop_vm(request)?;

        Ok(Uuid::parse_str(vm_id).unwrap())
    }

    #[allow(dead_code)]
    pub fn delete_all(&self, conn: &DbConnection) -> Result<usize, String> {
        match Vm::delete_all(conn) {
            Ok(record_count) => Ok(record_count),
            Err(e) => Err(e.to_string()),
        }
    }
}
