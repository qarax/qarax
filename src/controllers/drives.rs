use super::*;
use crate::database::DbConnection;
use crate::models::drive::NewDrive;
use crate::services::Backend;

#[get("/")]
pub fn index(backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend.drive_service.all(&conn) {
        Ok(drives) => ApiResponse {
            response: json!({ "drives": drives }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({"error": e.to_string()}),
            status: Status::BadRequest,
        },
    }
}

#[post("/", format = "json", data = "<drive>")]
pub fn add_drive(
    drive: Json<NewDrive>,
    backend: State<Backend>,
    conn: DbConnection,
) -> ApiResponse {
    match backend.drive_service.add(&drive.into_inner(), &conn) {
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
    routes![index, add_drive]
}
