use super::*;
use crate::models::kernel::{Kernel, NewKernel};
use crate::models::storage::Storage;

#[derive(Copy, Clone)]
pub struct KernelService {}

impl KernelService {
    pub fn new() -> Self {
        KernelService {}
    }

    pub fn all(&self, conn: &DbConnection) -> Result<Vec<Kernel>> {
        Kernel::all(conn)
    }

    pub fn add(&self, new_kernel: &NewKernel, conn: &DbConnection) -> Result<Uuid> {
        Kernel::insert(new_kernel, conn)
    }

    pub fn get_storage(&self, kernel_id: &str, conn: &DbConnection) -> Result<Storage> {
        Kernel::get_storage(Uuid::parse_str(kernel_id)?, conn)
    }
}
