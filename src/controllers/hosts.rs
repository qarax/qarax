use crate::database::Connection;
use crate::models::host::NewHost;
use crate::services::host as host_service;

use rocket_contrib::json::{Json, JsonValue};

#[get("/")]
pub fn index(conn: Connection) -> JsonValue {
    json!({ "hosts": host_service::get_all(conn) })
}

#[post("/", format = "json", data = "<host>")]
pub fn add_host(host: Json<NewHost>, conn: Connection) -> JsonValue {
    match host_service::add_host(host.into_inner(), conn) {
        Ok(id) => json!({ "host_id": id }),
        Err(e) => json!({ "error": e }),
    }
}
