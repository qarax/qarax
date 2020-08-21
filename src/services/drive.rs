use super::*;
use crate::models::drive::{Drive, NewDrive};
use crate::models::storage::{Storage, StorageConfig};
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

    pub fn get_storage(&self, drive_id: String, conn: &DbConnection) -> Result<Storage> {
        Drive::get_storage(Uuid::parse_str(&drive_id)?, conn)
    }
}

pub trait Volume {
    fn get_path(&self) -> String;
}

pub struct LocalVolume<'a> {
    drive_id: &'a str,
    storage: &'a StorageConfig,
}

impl<'a> LocalVolume<'a> {
    pub fn new(drive_id: &'a str, storage: &'a StorageConfig) -> LocalVolume<'a> {
        LocalVolume { drive_id, storage }
    }
}

impl<'a> Volume for LocalVolume<'a> {
    fn get_path(&self) -> String {
        format!("{}/{}", self.storage.path.as_ref().unwrap(), self.drive_id)
    }
}

pub struct SharedVolume {}
