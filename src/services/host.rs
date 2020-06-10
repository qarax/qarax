use crate::database::DbConnection;
use crate::models::host::{Host, NewHost};

pub fn get_all(conn: &DbConnection) -> Vec<Host> {
    Host::all(conn)
}

pub fn add_host(host: NewHost, conn: &DbConnection) -> Result<uuid::Uuid, String> {
    Host::insert(host, conn)
}

#[allow(dead_code)]
pub fn delete_all(conn: &DbConnection) -> Result<usize, String> {
    match Host::delete_all(conn) {
        Ok(record_count) => Ok(record_count),
        Err(e) => Err(e.to_string()),
    }
}
