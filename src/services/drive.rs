use super::*;
use crate::models::drive::{Drive, NewDrive};
use crate::models::vm::Vm;


#[derive(Copy, Clone)]
pub struct DriveService {}

impl DriveService {
    pub fn new() -> Self {
        DriveService {}
    }

    pub fn all(&self, conn: &DbConnection) -> Result<Vec<Drive>> {
        Drive::all(conn)
    }

    pub fn add(&self, new_drive: &NewDrive, conn: &DbConnection) -> Result<Uuid> {
        Drive::insert(new_drive, conn)
    }

    pub fn get_drives_for_vms(&self, vm: &Vm, conn: &DbConnection) -> Result<Vec<Drive>> {
        Drive::get_drives_for_vm(vm, conn)
    }
}
