use crate::database::Connection;
use crate::models::host::Host;
use rocket_contrib::json::Json;

#[get("/hosts")]
pub fn index(conn: Connection) -> Json<Vec<Host>> {
    let hosts = Host::all(conn);
    Json(hosts)
}
