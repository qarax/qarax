use super::*;
use crate::models::storage::{NewStorage, Storage};

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

    #[allow(dead_code)]
    pub fn delete_all(&self, conn: &DbConnection) -> Result<usize, String> {
        match Storage::delete_all(conn) {
            Ok(record_count) => Ok(record_count),
            Err(e) => Err(e.to_string()),
        }
    }
}
