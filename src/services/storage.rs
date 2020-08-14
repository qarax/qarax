use super::*;
use crate::models::storage::{Storage, NewStorage};

#[derive(Copy, Clone)]
pub struct StorageService {}

impl StorageService {
    pub fn new() -> Self {
        StorageService {}
    }

    pub fn all(&self, conn: &DbConnection) -> Result<Vec<Storage>> {
        Storage::all(conn)
    }

    pub fn add(&self, new_storage: &NewStorage, conn: &DbConnection) -> Result<Uuid> {
        Storage::insert(new_storage, conn)
    }
}
