use crate::database::DbConnection;
use crate::models::host::{Host, NewHost};

use super::util::ansible;
use std::collections::BTreeMap;

pub fn get_all(conn: &DbConnection) -> Vec<Host> {
    Host::all(conn)
}

pub fn add_host(host: &NewHost, conn: &DbConnection) -> Result<uuid::Uuid, String> {
    Host::insert(host, conn)
}

pub fn install(host: &NewHost) {
    let mut extra_params = BTreeMap::new();
    extra_params.insert("ansible_password", host.password.as_str());

    // TODO: make configurable
    extra_params.insert("fcversion", "0.21.1");

    let ac = ansible::AnsibleCommand::new(
        ansible::INSTALL_HOST_PLAYBOOK,
        &host.user,
        &host.address,
        &extra_params,
    );

    ac.run_playbook()
}

#[allow(dead_code)]
pub fn delete_all(conn: &DbConnection) -> Result<usize, String> {
    match Host::delete_all(conn) {
        Ok(record_count) => Ok(record_count),
        Err(e) => Err(e.to_string()),
    }
}
