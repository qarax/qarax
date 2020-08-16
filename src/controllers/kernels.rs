use super::*;
use crate::database::DbConnection;
use crate::models::kernel::NewKernel;
use crate::services::Backend;

#[get("/")]
pub fn index(backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend.kernel_service.all(&conn) {
        Ok(kernels) => ApiResponse {
            response: json!({ "kernels": kernels }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({"error": e.to_string()}),
            status: Status::BadRequest,
        },
    }
}

#[post("/", format = "json", data = "<kernel>")]
pub fn add_kernel(
    kernel: Json<NewKernel>,
    backend: State<Backend>,
    conn: DbConnection,
) -> ApiResponse {
    match backend.kernel_service.add(&kernel.into_inner(), &conn) {
        Ok(id) => ApiResponse {
            response: json!({ "kernel_id": id }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({ "error": e.to_string() }),
            status: Status::BadRequest,
        },
    }
}

#[get("/<id>/storage_config")]
pub fn get_storage_config(id: Uuid, backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend
        .kernel_service
        .get_storage_config(id.to_string(), &conn)
    {
        Ok(id) => ApiResponse {
            response: json!({ "kernel_id": id }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({ "error": e.to_string() }),
            status: Status::BadRequest,
        },
    }
}

pub fn routes() -> Vec<rocket::Route> {
    routes![index, add_kernel, get_storage_config]
}
