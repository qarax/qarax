use crate::database::Connection;
use crate::models::host::{Host, NewHost};
use rocket_contrib::json::{Json, JsonValue};

#[get("/hosts")]
pub fn index(conn: Connection) -> JsonValue {
    let hosts = Host::all(conn);
    json!({ "hosts": hosts })
}

#[post("/hosts", format = "json", data = "<host>")]
pub fn add_host(host: Json<NewHost>, conn: Connection) -> JsonValue {
    match Host::insert(host.into_inner(), conn) {
        Ok(id) => json!({ "host_id": id }),
        Err(e) => json!({ "error": e }),
    }
}
