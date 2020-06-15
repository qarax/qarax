use super::*;
use crate::database::DbConnection;
use crate::models::host::NewHost;
use crate::services::host::HostService;
use rocket_contrib::json::{Json, JsonValue};
use rocket_contrib::uuid::Uuid;

#[get("/")]
pub fn index(hs: State<HostService>, conn: DbConnection) -> JsonValue {
    json!({ "hosts": hs.get_all(&conn) })
}

#[get("/<id>")]
pub fn by_id(id: Uuid, hs: State<HostService>, conn: DbConnection) -> JsonValue {
    json!({ "host": hs.get_by_id(&id.to_string(), &conn) })
}

#[post("/", format = "json", data = "<host>")]
pub fn add_host(host: Json<NewHost>, hs: State<HostService>, conn: DbConnection) -> JsonValue {
    match hs.add_host(&host.into_inner(), &conn) {
        Ok(id) => json!({ "host_id": id }),
        Err(e) => json!({ "error": e }),
    }
}

#[get("/health/<id>")]
pub fn health_check(id: Uuid, hs: State<HostService>, conn: DbConnection) -> JsonValue {
    match hs.health_check(&id.to_string(), &conn) {
        Ok(status) => json!({ "host_status": status }),
        Err(status) => json!({ "host_status": status }),
    }
}

#[post("/install", format = "json", data = "<host>")]
pub fn install(host: Json<NewHost>, hs: State<HostService>, conn: DbConnection) {
    // TODO: error handling
    hs.install(&host, &conn);
}

pub fn routes() -> Vec<rocket::Route> {
    routes![index, by_id, add_host, install, health_check]
}

// TODO: Use some sort of lock, currently impossible to run tests in parallel
#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::ContentType;
    use rocket::local::Client;
    use rocket::State;
    use serde_json::Value;

    embed_migrations!();

    fn get_client() -> (HostService, Client, DbConnection) {
        let hs = HostService::new();
        let rocket = rocket::ignite()
            .manage(hs)
            .attach(DbConnection::fairing())
            .mount("/hosts", routes());

        let conn = DbConnection::get_one(&rocket).expect("Database connection failed");
        embedded_migrations::run(&*conn).expect("Failed to run migrations");
        let client = Client::new(rocket).expect("Failed to get client");

        (hs, client, conn)
    }

    #[test]
    fn test_index_empty() {
        let (hs, client, conn) = get_client();
        client.get("/hosts").dispatch();
        assert_eq!(hs.get_all(&conn).len(), 0);
    }

    #[test]
    fn test_index_one_result() {
        let payload = r#"{ 
        "name":"hosto",
        "address": "1.1.1.1",
        "user": "root",
        "password": "passwordo",
        "local_node_path": "/home/",
        "port": 8001}"#;

        let (hs, client, conn) = get_client();
        client
            .post("/hosts")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();
        let hs = HostService::new();

        assert_eq!(hs.get_all(&conn).len(), 1);

        // TODO: Stupid teardown
        hs.delete_all(&conn);
    }

    #[test]
    fn test_get_host_by_id() {
        let payload = r#"{ 
        "name":"hosto",
        "address": "1.1.1.1",
        "user": "root",
        "password": "passwordo",
        "local_node_path": "/home/",
        "port": 8001}"#;

        let (hs, client, conn) = get_client();
        let mut response = client
            .post("/hosts")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response = response.body_string();
        assert_eq!(response.is_some(), true);

        let response: Value = serde_json::from_str(&response.unwrap()).unwrap();
        let host_id = response["host_id"].as_str().unwrap();
        let hs = HostService::new();

        assert_eq!(hs.get_by_id(host_id, &conn).is_ok(), true);

        // TODO: Stupid teardown
        hs.delete_all(&conn);
    }
}
