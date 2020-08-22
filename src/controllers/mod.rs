pub mod drives;
pub mod hosts;
pub mod kernels;
pub mod storage;
pub mod vms;

use anyhow::Result;
use rocket::http::{ContentType, Status};
use rocket::request::Request;
use rocket::response;
use rocket::response::{Responder, Response};
use rocket::State;
use rocket_contrib::json::{Json, JsonValue};
use rocket_contrib::uuid::Uuid;

#[derive(Debug)]
pub struct ApiResponse {
    response: JsonValue,
    status: Status,
}

impl<'r> Responder<'r> for ApiResponse {
    fn respond_to(self, req: &Request) -> response::Result<'r> {
        Response::build_from(self.response.respond_to(&req).unwrap())
            .status(self.status)
            .header(ContentType::JSON)
            .ok()
    }
}
