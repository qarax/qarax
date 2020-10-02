use crate::database::DbConnection;
use crate::models::host::{Host, InstallHost, NewHost, Status};

use super::rpc::client::Client;
use super::util::ansible;

use super::*;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use dashmap::DashMap;

#[derive(Clone)]
pub struct HostService {
    clients: Arc<RwLock<HashMap<Uuid, Client>>>,
    locks: DashMap<Uuid, Arc<Mutex<()>>>,
}

impl HostService {
    pub fn new() -> Self {
        HostService {
            clients: Arc::new(RwLock::new(HashMap::new())),
            locks: DashMap::new(),
        }
    }

    pub fn get_by_id(&self, host_id: &str, conn: &DbConnection) -> Result<Host> {
        Host::by_id(Uuid::parse_str(host_id).unwrap(), conn)
    }

    pub fn get_all(&self, conn: &DbConnection) -> Result<Vec<Host>> {
        Host::all(conn)
    }

    pub fn add_host(&self, host: &NewHost, conn: &DbConnection) -> Result<Uuid> {
        Host::insert(host, conn)
    }

    pub fn install(&self, host_id: &str, host: &InstallHost, conn: DbConnection) -> Result<String> {
        let uuid = &Uuid::parse_str(host_id)?;

        let lock: Arc<Mutex<()>>;
        if let Some(ref v) = self.locks.get(uuid) {
            if let Ok(ref mut _m) = v.try_lock() {
                println!("lock for host '{}' acquired", host_id);
                lock = self.locks.insert(*uuid, Arc::clone(&v)).unwrap();
            } else {
                println!("lock for host '{}' is held", host_id);
                return Err(anyhow!("Host is currently being installed"));
            }
        } else {
            println!("creating a lock for host '{}'", host_id);
            lock = Arc::new(Mutex::new(()));
        }

        let m = lock.lock().unwrap();
        self.locks.insert(*uuid, Arc::clone(&lock));

        let db_host = self.get_by_id(host_id, &conn)?;
        if db_host.status == Status::Installing {
            std::mem::drop(m);
            self.locks.remove(uuid);
            return Err(anyhow!("Host is currently being installed"));
        }

        Host::update_status(&db_host, Status::Installing, &*conn).unwrap();
        println!("releasing lock for host '{}'", host_id);

        // Just in case?
        std::mem::drop(m);
        self.locks.remove(uuid);

        let mut extra_params = BTreeMap::new();
        extra_params.insert(
            String::from("ansible_password"),
            db_host.password.to_owned(),
        );

        // TODO: make configurable
        extra_params.insert(String::from("fcversion"), String::from("0.22.0"));
        extra_params.insert(
            String::from("local_node_path"),
            host.local_node_path.to_owned(),
        );

        let clients = Arc::clone(&self.clients);
        thread::spawn(move || {
            let ac = ansible::AnsibleCommand::new(
                ansible::INSTALL_HOST_PLAYBOOK,
                &db_host.host_user,
                &db_host.address,
                extra_params,
            );

            if let Err(e) = ac.run_playbook().with_context(|| "Couldn't run playbook") {
                eprintln!("{:?}", e);
                Host::update_status(&db_host, Status::Down, &*conn).expect("Failed to update host");
                return;
            }

            match Client::connect(format!("http://{}:{}", db_host.address, db_host.port)) {
                Err(e) => {
                    eprintln!("Failed to connect to qarax-node, {:?}", e);
                    Host::update_status(&db_host, Status::Down, &*conn)
                        .expect("Failed to update host");
                }
                Ok(c) => {
                    println!("Successfully connected to qarax-node");
                    clients.write().unwrap().insert(db_host.id, c.clone());
                    if Self::health_check_internal(&c).is_err() {
                        eprintln!("Health check failed, failing installation");
                        Host::update_status(&db_host, Status::Down, &*conn)
                            .expect("Failed to update host");
                    } else {
                        println!("Finished installation of {}", db_host.id);
                        Host::update_status(&db_host, Status::Up, &*conn)
                            .expect("Failed to update host");
                    }
                }
            }
        });

        Ok(String::from("installing host"))
    }

    pub fn health_check(&self, host_id: &str) -> Result<String> {
        match self
            .clients
            .read()
            .unwrap()
            .get(&Uuid::parse_str(host_id).unwrap())
        {
            Some(c) => Self::health_check_internal(c),
            None => Err(anyhow!("No client found for host")),
        }
    }

    fn health_check_internal(client: &Client) -> Result<String> {
        use tonic::Request;

        let response = client.health_check(Request::new(()));
        match response {
            Ok(_) => Ok(String::from("OK")),
            Err(e) => Err(anyhow!("Failed {:?}", e)),
        }
    }

    pub fn get_client(&self, host_id: Uuid) -> Result<Client> {
        // TODO: error handling
        let client = self.clients.read().unwrap();
        match client.get(&host_id) {
            Some(client) => Ok(client.clone()),
            None => Err(anyhow!("Client unavailable for host {}", host_id)),
        }
    }

    pub fn get_running_host(&self, conn: &DbConnection) -> Result<Host> {
        let hosts = Host::by_status(Status::Up, conn)?;
        match hosts.first() {
            Some(h) => Ok(h.clone()),
            None => Err(anyhow!("No hosts available")),
        }
    }

    pub fn initialize_hosts(&self, conn: &DbConnection) {
        let hosts = Host::by_status(Status::Up, conn).unwrap();
        hosts.iter().for_each(|host| {
            // TODO: this can and should be done concurrently
            match Client::connect(format!("http://{}:{}", host.address, host.port)) {
                Ok(client) => {
                    println!("Saving client for host {}", host.id);
                    self.clients
                        .write()
                        .unwrap()
                        .insert(host.id, client.clone());

                    match Self::health_check_internal(&client) {
                        Ok(_) => {
                            println!("Successfully initialized host {}", host.id,);
                        }
                        Err(e) => {
                            eprintln!("Host is unhealthy {}, error: {}", host.id, e);

                            // TODO: down status is not correct, need to introduce a new status
                            // for this
                            Host::update_status(host, Status::Down, &*conn)
                                .expect("Failed to update host");
                        }
                    };
                }
                Err(e) => {
                    eprintln!("Could not connect to host {}, error: {}", host.id, e);
                    Host::update_status(host, Status::Down, &*conn).expect("Failed to update host");
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
