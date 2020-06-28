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
        Err(_) => json!({ "error": "could not start vm" }),
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
