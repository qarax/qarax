use crate::models::host::Host;
use rocket_contrib::json::Json;
use crate::database::Connection;

#[get("/hosts")]
pub fn index(conn: Connection) -> Json<Vec<Host>> {
    let hosts = Host::all(conn);
    Json(hosts)
}