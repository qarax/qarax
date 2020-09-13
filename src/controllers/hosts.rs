use super::*;
use crate::database::DbConnection;
use crate::models::host::{InstallHost, NewHost};
use crate::services::Backend;

#[get("/")]
pub fn index(backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend.host_service.get_all(&conn) {
        Ok(hosts) => ApiResponse {
            response: json!({ "hosts": hosts }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({"error": e.to_string()}),
            status: Status::BadRequest,
        },
    }
}

#[get("/<id>")]
pub fn by_id(id: Uuid, backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend.host_service.get_by_id(&id.to_string(), &conn) {
        Ok(h) => ApiResponse {
            response: json!({ "host": h }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({"error": e.to_string()}),
            status: Status::NotFound,
        },
    }
}

#[post("/", format = "json", data = "<host>")]
pub fn add_host(host: Json<NewHost>, backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend.host_service.add_host(&host.into_inner(), &conn) {
        Ok(id) => ApiResponse {
            response: json!({ "host_id": id }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({ "error": e.to_string() }),
            status: Status::BadRequest,
        },
    }
}

#[get("/<id>/health")]
pub fn health_check(id: Uuid, backend: State<Backend>) -> ApiResponse {
    match backend.host_service.health_check(&id.to_string()) {
        Ok(status) => ApiResponse {
            response: json!({ "host_status": status }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({ "host_status": e.to_string() }),
            status: Status::BadRequest,
        },
    }
}

#[post("/<host_id>/install", format = "json", data = "<host>")]
pub fn install(
    host_id: Uuid,
    host: Json<InstallHost>,
    backend: State<Backend>,
    conn: DbConnection,
) -> ApiResponse {
    match backend
        .host_service
        .clone()
        .install(&host_id.to_string(), &host, conn)
    {
        Ok(status) => ApiResponse {
            response: json!({ "status": status }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({ "error": e.to_string()}),
            status: Status::BadRequest,
        },
    }
}

pub fn routes() -> Vec<rocket::Route> {
    routes![index, by_id, add_host, install, health_check]
}

// TODO: Use some sort of lock, currently impossible to run tests in parallel
#[cfg(test)]
mod tests {
    use super::*;
    use crate::common;

    use rocket::http::ContentType;
    use serde_json::Value;

    const MOUNT: &str = "/hosts";

    embed_migrations!();

    #[test]
    fn test_index_empty() {
        let (client, _) = common::get_client(MOUNT, routes());
        let mut response = client.get("/hosts").dispatch();

        let response = response.body_string();
        let response: Value = serde_json::from_str(&response.unwrap()).unwrap();

        assert_eq!(response.to_string(), "{\"hosts\":[]}");
    }

    #[test]
    fn test_index_one_result() {
        let payload = r#"{ 
        "name":"hosto",
        "address": "1.1.1.1",
        "host_user": "root",
        "password": "passwordo",
        "port": 8001}"#;

        let (client, conn) = common::get_client(MOUNT, routes());
        client
            .post("/hosts")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let backend: State<Backend> = State::from(&client.rocket()).unwrap();

        assert_eq!(backend.host_service.get_all(&conn).unwrap().len(), 1);

        // TODO: Stupid teardown
        backend.host_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_get_host_by_id() {
        let payload = r#"{ 
        "name":"hosto",
        "address": "1.1.1.1",
        "host_user": "root",
        "password": "passwordo",
        "port": 8001}"#;

        let (client, conn) = common::get_client(MOUNT, routes());
        let mut response = client
            .post("/hosts")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response = response.body_string();
        assert_eq!(response.is_some(), true);

        let response: Value = serde_json::from_str(&response.unwrap()).unwrap();
        let host_id = response["host_id"].as_str().unwrap();

        let backend: State<Backend> = State::from(&client.rocket()).unwrap();

        assert_eq!(backend.host_service.get_by_id(host_id, &conn).is_ok(), true);

        // TODO: Stupid teardown
        backend.host_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_host_not_found() {
        let (client, _) = common::get_client(MOUNT, routes());
        let response = client
            .get("/hosts/835b6b42-9e70-43ef-a58d-6235ab0e1495")
            .dispatch();

        assert_eq!(response.status(), Status::NotFound);
    }

    #[test]
    fn test_host_duplicate_name() {
        let payload = r#"{
            "name":"hosto",
            "address": "1.1.1.1",
            "host_user": "root",
            "password": "passwordo",
            "port": 8001}"#;

        let (client, conn) = common::get_client(MOUNT, routes());
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();

        let response1 = client
            .post("/hosts")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        assert_eq!(response1.status(), Status::Ok);

        let response2 = client
            .post("/hosts")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        assert_eq!(response2.status(), Status::BadRequest);

        // TODO: Stupid teardown
        backend.host_service.delete_all(&conn).unwrap();
    }
}
