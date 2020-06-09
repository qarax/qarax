use crate::database::Connection;
use crate::models::host::{Host, NewHost};

pub fn get_all(conn: Connection) -> Vec<Host> {
    return Host::all(conn);
}

pub fn add_host(host: NewHost, conn: Connection) -> Result<uuid::Uuid, String> {
    Host::insert(host, conn)
}
