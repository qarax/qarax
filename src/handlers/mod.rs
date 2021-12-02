use crate::env::Environment;

use axum::{
    body::BoxBody,
    extract::Extension,
    response::{self, IntoResponse},
    routing::{get, post},
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

pub async fn initialize_env(env: Environment) {
    tracing::info!("Initializing hosts...");

    match hosts::initalize_hosts(env).await {
        Ok(_) => (),
        Err(e) => {
            tracing::error!("Failed to initialize hosts: {}", e);
        }
    }

    tracing::info!("Finished initializing hosts");
}

pub async fn app(env: Environment) -> Router {
    initialize_env(env.clone()).await;
    Router::new()
        .route("/", get(|| async { "hello" }))
        .nest("/hosts", hosts())
        .nest("/storage", storage())
        .nest("/drives", drives())
        .nest("/kernels", kernels())
        .nest("/vms", vms())
        .layer(AddExtensionLayer::new(env.clone()))
        .layer(tower_http::trace::TraceLayer::new_for_http())
}

pub fn hosts() -> Router {
    Router::new()
        .route("/:id", get(hosts::get))
        .route("/:id/install", post(hosts::install))
        .route("/:id/healthcheck", post(hosts::health_check))
        .route("/", get(hosts::list).post(hosts::add))
}

pub fn storage() -> Router {
    Router::new()
        .route("/:id", get(storage::get))
        .route("/", get(storage::list).post(storage::add))
}

pub fn drives() -> Router {
    Router::new()
        .route("/:id", get(drives::get))
        .route("/", get(drives::list).post(drives::add))
}

pub fn kernels() -> Router {
    Router::new()
        .route("/:id", get(kernels::get))
        .route("/", get(kernels::list).post(kernels::add))
}

pub fn vms() -> Router {
    Router::new()
        .route("/:id", get(vms::get))
        .route("/", get(vms::list).post(vms::add))
        .route("/:id/start", post(vms::start))
}

pub struct ApiResponse<T> {
    data: T,
    code: StatusCode,
}

impl<T> IntoResponse for ApiResponse<T>
where
    T: Send + Sync + Serialize,
{
    fn into_response(self) -> Response<BoxBody> {
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
    Internal(String),
    #[error("Validation error")]
    #[serde(rename(serialize = "validation error"))]
    Validation(String),
    #[error("Entity not found")]
    #[serde(rename(serialize = "entity not found"))]
    EntityNotFound(String),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response<BoxBody> {
        let code = match self {
            ServerError::Internal(ref s) => {
                tracing::error!("Internal error: {}", s);
                StatusCode::INTERNAL_SERVER_ERROR
            }
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
