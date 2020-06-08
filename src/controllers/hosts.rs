use crate::database::Connection;
use crate::models::host::{NewHost, Host};
use rocket_contrib::json::{JsonValue, Json};

#[get("/hosts")]
pub fn index(conn: Connection) -> JsonValue {
    let hosts = Host::all(conn);
    json!({"hosts": hosts})
}

#[post("/hosts", format = "json", data = "<host>")]
pub fn add_host(host: Json<NewHost>, conn: Connection) -> JsonValue {
    match Host::insert(&host, conn) {
        Ok(id) => json!({"host_id": id}),
        Err(e) => json!({"error": e})
    }
}
