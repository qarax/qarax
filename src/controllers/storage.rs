use super::*;
use crate::database::DbConnection;
use crate::models::storage::NewStorage;
use crate::services::Backend;

#[get("/")]
pub fn index(backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend.storage_service.all(&conn) {
        Ok(storages) => ApiResponse {
            response: json!({ "storages": storages }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({"error": e.to_string()}),
            status: Status::BadRequest,
        },
    }
}

#[post("/", format = "json", data = "<storage>")]
pub fn add_storage(
    storage: Json<NewStorage>,
    backend: State<Backend>,
    conn: DbConnection,
) -> ApiResponse {
    match backend.storage_service.add(&storage.into_inner(), &conn) {
        Ok(id) => ApiResponse {
            response: json!({ "id": id }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({ "error": e.to_string() }),
            status: Status::BadRequest,
        },
    }
}

pub fn routes() -> Vec<rocket::Route> {
    routes![index, add_storage]
}
