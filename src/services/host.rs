use crate::database::DbConnection;
use crate::models::host::{Host, InstallHost, NewHost, Status};

use super::rpc::client::Client;
use super::util::ansible;

use std::collections::BTreeMap;
use std::collections::HashMap;

use std::thread;

use std::sync::Arc;
use std::sync::RwLock;
use uuid::Uuid;

#[derive(Clone)]
pub struct HostService {
    clients: Arc<RwLock<HashMap<Uuid, Client>>>,
}

impl HostService {
    pub fn new() -> Self {
        HostService {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_by_id(&self, host_id: &str, conn: &DbConnection) -> Result<Host, String> {
        Host::by_id(Uuid::parse_str(host_id).unwrap(), conn)
    }

    pub fn get_all(&self, conn: &DbConnection) -> Vec<Host> {
        Host::all(conn)
    }

    pub fn add_host(&self, host: &NewHost, conn: &DbConnection) -> Result<Uuid, String> {
        Host::insert(host, conn)
    }

    pub fn install(
        self,
        host_id: &str,
        host: &InstallHost,
        conn: DbConnection,
    ) -> Result<String, String> {
        let db_host = match self.get_by_id(host_id, &conn) {
            Ok(h) => h,
            Err(_) => return Err(String::from("Host not found")),
        };

        let mut extra_params = BTreeMap::new();
        extra_params.insert(
            String::from("ansible_password"),
            db_host.password.to_owned(),
        );

        // TODO: make configurable
        extra_params.insert(String::from("fcversion"), String::from("0.21.1"));
        extra_params.insert(
            String::from("local_node_path"),
            host.local_node_path.to_owned(),
        );

        thread::spawn(move || {
            // TODO: handle errors
            Host::update_status(&db_host, Status::Installing, &*conn);

            let ac = ansible::AnsibleCommand::new(
                ansible::INSTALL_HOST_PLAYBOOK,
                &db_host.host_user,
                &db_host.address,
                extra_params,
            );

            ac.run_playbook();

            match Client::connect(format!("http://{}:{}", db_host.address, db_host.port)) {
                Ok(c) => {
                    println!("Successfully connected to qarax-node");
                    self.clients.write().unwrap().insert(db_host.id, c);
                }
                Err(e) => {
                    println!("Failed to connect to qarax-node {}", e.to_string());
                    Host::update_status(&db_host, Status::Down, &*conn);
                }
            }

            // TODO: fail instead of just printing
            match self.health_check(&db_host.id.to_string()) {
                Ok(r) => {
                    println!("Health check: {}", r);
                    Host::update_status(&db_host, Status::Up, &*conn);
                    println!("Finished installation of {}", db_host.id);
                }
                Err(e) => {
                    Host::update_status(&db_host, Status::Down, &*conn);
                    eprintln!("Health check failed, failing installation")
                }
            };
        });

        Ok(String::from("installing host"))
    }

    pub fn health_check(&self, host_id: &str) -> Result<String, String> {
        use tonic::Request;

        match self
            .clients
            .read()
            .unwrap()
            .get(&Uuid::parse_str(host_id).unwrap())
        {
            Some(c) => {
                let response = c.health_check(Request::new(()));
                return match response {
                    Ok(_) => Ok(String::from("OK")),
                    Err(_) => Err(String::from("ERROR")),
                };
            }
            None => Err(String::from("No client found for host")),
        }
    }

    pub fn get_client(&self, host_id: Uuid) -> Client {
        // TODO: error handling
        self.clients.read().unwrap().get(&host_id).cloned().unwrap()
    }

    pub fn get_running_host(&self, conn: &DbConnection) -> Host {
        // TODO: error handling
        Host::by_status(Status::Up, conn)
            .unwrap()
            .first()
            .cloned()
            .unwrap()
    }

    pub fn initialize_hosts(&self, conn: &DbConnection) {
        let hosts = Host::by_status(Status::Up, conn).unwrap();
        hosts.iter().for_each(|host| {
            // TODO: this can and should be done concurrently
            match Client::connect(format!("http://{}:{}", host.address, host.port)) {
                Ok(c) => {
                    println!("Saving client for host {}", host.id);
                    self.clients.write().unwrap().insert(host.id, c);

                    match self.health_check(&host.id.to_string()) {
                        Ok(_) => {
                            println!("Successfully initialized host {}", host.id,);
                        }
                        Err(e) => {
                            eprintln!("Host is unhealthy {}, error: {}", host.id, e);

                            // TODO: down status is not correct, need to introduce a new status
                            // for this
                            Host::update_status(host, Status::Down, &*conn);
                            return ();
                        }
                    };
                }
                Err(e) => {
                    eprintln!("Could not connect to host {}, error: {}", host.id, e);
                    Host::update_status(host, Status::Down, &*conn);
                    return ();
                }
            };
        });
    }

    #[allow(dead_code)]
    pub fn delete_all(&self, conn: &DbConnection) -> Result<usize, String> {
        match Host::delete_all(conn) {
            Ok(record_count) => Ok(record_count),
            Err(e) => Err(e.to_string()),
        }
    }
}
