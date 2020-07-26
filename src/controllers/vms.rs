use super::*;
use crate::database::DbConnection;
use crate::models::vm::NewVm;
use crate::services::Backend;

use rocket_contrib::json::{Json, JsonValue};
use rocket_contrib::uuid::Uuid;

#[get("/")]
pub fn index(backend: State<Backend>, conn: DbConnection) -> JsonValue {
    json!({ "vms": backend.vm_service.get_all(&conn) })
}

#[get("/<id>")]
pub fn by_id(id: Uuid, backend: State<Backend>, conn: DbConnection) -> JsonValue {
    json!({ "vm": backend.vm_service.get_by_id(&id.to_string(), &conn) })
}

#[post("/", format = "json", data = "<vm>")]
pub fn add_vm(vm: Json<NewVm>, backend: State<Backend>, conn: DbConnection) -> JsonValue {
    match backend.vm_service.add_vm(&vm.into_inner(), &conn) {
        Ok(id) => json!({ "vm_id": id }),
        Err(e) => json!({ "error": e }),
    }
}

#[post("/<id>/start")]
pub fn start_vm(id: Uuid, backend: State<Backend>, conn: DbConnection) -> JsonValue {
    match backend
        .vm_service
        .start(&id.to_string(), &backend.host_service, &conn)
    {
        Ok(id) => json!({ "vm_id": id }),
        Err(e) => json!({ "error": format!("could not start vm: {}", e) }),
    }
}

#[post("/<id>/stop")]
pub fn stop_vm(id: Uuid, backend: State<Backend>, conn: DbConnection) -> JsonValue {
    match backend
        .vm_service
        .stop(&id.to_string(), &backend.host_service, &conn)
    {
        Ok(id) => json!({ "vm_id": id }),
        Err(_) => json!({ "error": "could not stop vm" }),
    }
}

pub fn routes() -> Vec<rocket::Route> {
    routes![index, by_id, add_vm, start_vm, stop_vm]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::host::HostService;
    use crate::services::vm::VmService;

    use rocket::http::ContentType;
    use rocket::local::Client;
    use serde_json::Value;

    embed_migrations!();

    fn get_client() -> (Client, DbConnection) {
        let hs = HostService::new();
        let vs = VmService::new();
        let rocket = rocket::ignite()
            .manage(Backend {
                host_service: hs,
                vm_service: vs,
            })
            .attach(DbConnection::fairing())
            .mount("/vms", routes());

        let conn = DbConnection::get_one(&rocket).expect("Database connection failed");
        embedded_migrations::run(&*conn).expect("Failed to run migrations");
        let client = Client::new(rocket).expect("Failed to get client");
        (client, conn)
    }

    #[test]
    fn test_index_empty() {
        let (client, _) = get_client();
        let mut response = client.get("/vms").dispatch();

        let response = response.body_string();
        let response: Value = serde_json::from_str(&response.unwrap()).unwrap();

        assert_eq!(response.to_string(), "{\"vms\":[]}");
    }

    #[test]
    fn test_add_vm_no_network() {
        let payload = r#"{
            "name": "vm1",
            "vcpu": 1,
            "memory": 128,
            "kernel": "vmlinux",
            "root_file_system": "rootfs"
            }"#;

        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();

        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(vm.network_mode, None);

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_add_vm_dhcp_network() {
        let payload = r#"{
            "name": "vm1",
            "vcpu": 1,
            "memory": 128,
            "kernel": "vmlinux",
            "root_file_system": "rootfs",
            "network_mode": "dhcp"
            }"#;

        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();

        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(vm.network_mode, Some(String::from("dhcp")));

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_add_vm_static_ip_network() {
        let payload = r#"{
            "name": "vm1",
            "vcpu": 1,
            "memory": 128,
            "kernel": "vmlinux",
            "root_file_system": "rootfs",
            "network_mode": "static_ip",
            "address": "192.168.122.100"
            }"#;

        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();

        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(vm.network_mode, Some(String::from("static_ip")));
        assert_eq!(vm.address, Some(String::from("192.168.122.100")));

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_default_kernel_params() {
        let payload = r#"{
            "name": "vm1",
            "vcpu": 1,
            "memory": 128,
            "kernel": "vmlinux",
            "root_file_system": "rootfs",
            "network_mode": "static_ip",
            "address": "192.168.122.100"
            }"#;

        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();

        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(
            vm.kernel_params,
            String::from("console=ttyS0 reboot=k panic=1 pci=off")
        );

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_custom_kernel_params() {
        let payload = r#"{
            "name": "vm1",
            "vcpu": 1,
            "memory": 128,
            "kernel": "vmlinux",
            "root_file_system": "rootfs",
            "network_mode": "static_ip",
            "address": "192.168.122.100",
            "kernel_params": "ip=1.1.1.1"
            }"#;

        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();

        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(vm.kernel_params, String::from("ip=1.1.1.1"));

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
    }
}
