use super::*;
use crate::models::drive::{Drive, NewDrive};

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
}
