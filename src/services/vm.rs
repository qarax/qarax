use crate::database::DbConnection;
use crate::models::vm::{NewVm, Vm};

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
}
