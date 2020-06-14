use crate::database::DbConnection;
use crate::models::host::{Host, NewHost};

use super::rpc::client::Client;
use super::util::ansible;

use std::collections::BTreeMap;
use uuid::Uuid;

pub fn get_by_id(host_id: &str, conn: &DbConnection) -> Result<Host, String> {
    Host::by_id(Uuid::parse_str(host_id).unwrap(), conn)
}

pub fn get_all(conn: &DbConnection) -> Vec<Host> {
    Host::all(conn)
}

pub fn add_host(host: &NewHost, conn: &DbConnection) -> Result<Uuid, String> {
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

    ac.run_playbook();
}

pub fn health_check(host_id: &str, conn: &DbConnection) -> Result<String, String> {
    let host = match get_by_id(host_id, conn) {
        Ok(h) => h,
        Err(e) => return Err(e.to_string()),
    };

    use tonic::Request;

    let client = Client::connect(format!("http://{}:{}", host.address, host.port));
    match client {
        Ok(mut c) => {
            let response = c.health_check(Request::new(()));
            match response {
                Ok(_) => Ok(String::from("OK")),
                Err(_) => Err(String::from("ERROR")),
            }
        }
        Err(e) => Err(String::from(format!(
            "Could not connect: {}",
            e.to_string()
        ))),
    }
}

#[allow(dead_code)]
pub fn delete_all(conn: &DbConnection) -> Result<usize, String> {
    match Host::delete_all(conn) {
        Ok(record_count) => Ok(record_count),
        Err(e) => Err(e.to_string()),
    }
}
