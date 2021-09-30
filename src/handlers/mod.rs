use std::convert::Infallible;

use crate::env::Environment;

use axum::{
    body::{Bytes, Full},
    extract::Extension,
    handler::{get, post},
    response::{self, IntoResponse},
    routing::BoxRoute,
    AddExtensionLayer, Router,
};
use http::{Response, StatusCode};
use serde::Serialize;
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

mod ansible;
pub mod drives;
pub mod hosts;
pub mod kernels;
mod models;
pub mod rpc;
pub mod storage;
pub mod vms;

pub fn app(env: Environment) -> Router<BoxRoute> {
    Router::new()
        .route("/", get(|| async { "hello" }))
        .nest("/hosts", hosts())
        .nest("/storage", storage())
        .nest("/drives", drives())
        .nest("/kernels", kernels())
        .nest("/vms", vms())
        .layer(AddExtensionLayer::new(env))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .boxed()
}

pub fn hosts() -> Router<BoxRoute> {
    Router::new()
        .route("/:id", get(hosts::get))
        .route("/:id/install", post(hosts::install))
        .route("/:id/healthcheck", post(hosts::health_check))
        .route("/", get(hosts::list).post(hosts::add))
        .boxed()
}

pub fn storage() -> Router<BoxRoute> {
    Router::new()
        .route("/:id", get(storage::get))
        .route("/", get(storage::list).post(storage::add))
        .boxed()
}

pub fn drives() -> Router<BoxRoute> {
    Router::new()
        .route("/:id", get(drives::get))
        .route("/", get(drives::list).post(drives::add))
        .boxed()
}

pub fn kernels() -> Router<BoxRoute> {
    Router::new()
        .route("/:id", get(kernels::get))
        .route("/", get(kernels::list).post(kernels::add))
        .boxed()
}

pub fn vms() -> Router<BoxRoute> {
    Router::new()
        .route("/:id", get(vms::get))
        .route("/", get(vms::list).post(vms::add))
        .route("/:id/start", post(vms::start))
        .boxed()
}

pub struct ApiResponse<T> {
    data: T,
    code: StatusCode,
}

impl<T> IntoResponse for ApiResponse<T>
where
    T: Send + Sync + Serialize,
{
    type Body = Full<Bytes>;
    type BodyError = Infallible;

    fn into_response(self) -> Response<Self::Body> {
        let mut response = response::Json(json!({
            "response": self.data,
        }))
        .into_response();

        *response.status_mut() = self.code;
        response
    }
}

#[derive(Error, Debug, Serialize)]
pub enum ServerError {
    #[error("Internal error")]
    #[serde(rename(serialize = "internal error"))]
    Internal,
    #[error("Validation error")]
    #[serde(rename(serialize = "validation error"))]
    Validation(String),
    #[error("Entity not found")]
    #[serde(rename(serialize = "entity_not_found"))]
    EntityNotFound(String),
}

impl IntoResponse for ServerError {
    type Body = Full<Bytes>;
    type BodyError = Infallible;

    fn into_response(self) -> Response<Self::Body> {
        let code = match self {
            ServerError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::Validation(_) => StatusCode::CONFLICT,
            Self::EntityNotFound(_) => StatusCode::NOT_FOUND,
        };

        let mut response = response::Json(json!({
            "error": self,
        }))
        .into_response();
        *response.status_mut() = code;

        response
    }
}
