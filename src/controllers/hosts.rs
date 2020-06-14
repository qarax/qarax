use crate::database::DbConnection;
use crate::models::host::NewHost;
use crate::services::host as host_service;
use rocket_contrib::json::{Json, JsonValue};
use rocket_contrib::uuid::Uuid;

#[get("/")]
pub fn index(conn: DbConnection) -> JsonValue {
    json!({ "hosts": host_service::get_all(&conn) })
}

#[get("/<id>")]
pub fn by_id(id: Uuid, conn: DbConnection) -> JsonValue {
    json!({ "host": host_service::get_by_id(&id.to_string(), &conn) })
}

#[post("/", format = "json", data = "<host>")]
pub fn add_host(host: Json<NewHost>, conn: DbConnection) -> JsonValue {
    match host_service::add_host(&host.into_inner(), &conn) {
        Ok(id) => json!({ "host_id": id }),
        Err(e) => json!({ "error": e }),
    }
}

#[post("/install", format = "json", data = "<host>")]
pub fn install(host: Json<NewHost>) {
    // TODO: error handling
    host_service::install(&host);
}

pub fn routes() -> Vec<rocket::Route> {
    routes![index, by_id, add_host, install]
}

// TODO: Use some sort of lock, currently impossible to run tests in parallel
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::host as host_service;
    use rocket::http::ContentType;
    use rocket::local::Client;
    use serde_json::Value;

    embed_migrations!();

    fn get_client() -> (Client, DbConnection) {
        let rocket = rocket::ignite()
            .attach(DbConnection::fairing())
            .mount("/hosts", routes());
        let conn = DbConnection::get_one(&rocket).expect("Database connection failed");
        embedded_migrations::run(&*conn).expect("Failed to run migrations");
        let client = Client::new(rocket).expect("Failed to get client");
        (client, conn)
    }

    #[test]
    fn test_index_empty() {
        let (client, conn) = get_client();
        client.get("/hosts").dispatch();

        assert_eq!(host_service::get_all(&conn).len(), 0);
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

        let (client, conn) = get_client();
        client
            .post("/hosts")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        assert_eq!(host_service::get_all(&conn).len(), 1);

        // TODO: Stupid teardown
        host_service::delete_all(&conn);
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

        let (client, conn) = get_client();
        let mut response = client
            .post("/hosts")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response = response.body_string();
        assert_eq!(response.is_some(), true);

        let response: Value = serde_json::from_str(&response.unwrap()).unwrap();
        let host_id = response["host_id"].as_str().unwrap();
        assert_eq!(host_service::get_by_id(host_id, &conn).is_ok(), true);

        // TODO: Stupid teardown
        host_service::delete_all(&conn);
    }
}
