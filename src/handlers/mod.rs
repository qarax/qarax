use crate::env::Environment;

use axum::{
    body::BoxBody,
    response::{self, IntoResponse},
    routing::{get, post},
    Extension, Router,
};
use http::{header::HeaderName, Method, Request, Response, StatusCode};
use hyper::Body;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    request_id::{MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::TraceLayer,
};
use uuid::Uuid;

mod ansible;
pub mod drives;
pub mod hosts;
pub mod rpc;
pub mod storage;
pub mod vms;
pub mod volumes;

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
    let x_request_id = HeaderName::from_static("x-request-id");
    Router::new()
        .route("/", get(|| async { "hello" }))
        .nest("/hosts", hosts())
        .nest("/storage", storage())
        .nest("/vms", vms())
        .nest("/drives", drives())
        .layer(
            ServiceBuilder::new()
                .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
                .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
                .layer(
                    TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
                        let request_id = request
                            .extensions()
                            .get::<RequestId>()
                            .and_then(|id| id.header_value().to_str().ok())
                            .unwrap_or_default();

                        tracing::info_span!(
                            "HTTP",
                            http.method = %request.method(),
                            http.url = %request.uri(),
                            request_id = %request_id,
                        )
                    }),
                )
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(vec![Method::GET]),
                )
                .layer(Extension(env)),
        )
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
        .route("/:id", get(storage::handler::get))
        .route("/", get(storage::handler::list).post(storage::handler::add))
        .route("/:id/volumes", post(storage::handler::create_volume))
}

pub fn vms() -> Router {
    Router::new()
        .route("/:id", get(vms::get))
        .route("/", get(vms::list).post(vms::add))
        .route("/:id/start", post(vms::start))
}

pub fn drives() -> Router {
    Router::new()
        .route("/:id", get(drives::get))
        .route("/", get(drives::list).post(drives::add))
}

#[derive(Clone, Copy)]
struct MakeRequestUuid;

impl MakeRequestId for MakeRequestUuid {
    fn make_request_id<B>(&mut self, _: &Request<B>) -> Option<RequestId> {
        let request_id = Uuid::new_v4().to_string().parse().unwrap();
        Some(RequestId::new(request_id))
    }
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
    Internal(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Entity not found")]
    #[serde(rename(serialize = "entity not found"))]
    EntityNotFound(String),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response<BoxBody> {
        let code = match self {
            ServerError::Internal(_) => {
                tracing::error!("Internal error");
                StatusCode::INTERNAL_SERVER_ERROR
            }
            ServerError::Validation(ref e) => {
                tracing::error!("Validation error: {}", e);
                StatusCode::CONFLICT
            }
            Self::EntityNotFound(ref e) => {
                tracing::error!("Entity not found: {}", e);
                StatusCode::NOT_FOUND
            }
        };

        let mut response = response::Json(json!({
            "error": self,
        }))
        .into_response();
        *response.status_mut() = code;

        response
    }
}
