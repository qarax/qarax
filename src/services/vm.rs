use crate::database::DbConnection;
use crate::models::vm::{NewVm, Vm};
use crate::services::host::HostService;
use uuid::Uuid;

#[derive(Copy, Clone)]
pub struct VmService {}

impl VmService {
    pub fn new() -> Self {
        VmService {}
    }

    pub fn get_by_id(&self, vm_id: &str, conn: &DbConnection) -> Result<Vm, String> {
        Vm::by_id(Uuid::parse_str(vm_id).unwrap(), conn)
    }

    pub fn get_all(&self, conn: &DbConnection) -> Vec<Vm> {
        Vm::all(conn)
    }

    pub fn add_vm(&self, vm: &NewVm, conn: &DbConnection) -> Result<Uuid, String> {
        Vm::insert(vm, conn)
    }

    pub fn start(
        &self,
        vm_id: &str,
        host_service: &HostService,
        conn: &DbConnection,
    ) -> Result<Uuid, String> {
        use super::rpc::client::node::VmConfig;

        // TODO: error handling
        let host = host_service.get_running_host(conn);
        let client = host_service.get_client(host.id);
        let vm = self.get_by_id(vm_id, conn).unwrap();

        let request = VmConfig {
            vm_id: vm.id.to_string(),
            memory: vm.memory,
            vcpus: vm.vcpu,
            kernel: vm.kernel,
            root_fs: vm.root_file_system,
        };

        client.start_vm(request);

        Ok(vm.id)
    }

    pub fn stop(
        &self,
        vm_id: &str,
        host_service: &HostService,
        conn: &DbConnection,
    ) -> Result<Uuid, String> {
        use super::rpc::client::node::VmId;

        // TODO: error handling
        let host = host_service.get_running_host(conn);
        let client = host_service.get_client(host.id);
        let request = VmId {
            vm_id: vm_id.to_owned(),
        };
        client.stop_vm(request);

        Ok(Uuid::parse_str(vm_id).unwrap())
    }
}
