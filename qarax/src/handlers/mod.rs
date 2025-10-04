use crate::{App, errors::Error};
use axum::{
    Extension, Json, Router,
    body::Body,
    response::{self, IntoResponse, Response},
    routing::get,
};
use http::{Request, StatusCode, header::HeaderName};
use serde::Serialize;
use serde_json::json;
use serde_with::DisplayFromStr;
use tower::ServiceBuilder;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::TraceLayer,
};
use validator::ValidationErrors;

mod host;
mod vm;

pub type Result<T, E = Error> = ::std::result::Result<T, E>;

pub fn app(env: App) -> Router {
    let x_request_id = HeaderName::from_static("x-request-id");
    Router::new()
        .route("/", get(|| async { "hello" }))
        .merge(hosts())
        .merge(vms())
        .layer(
            ServiceBuilder::new()
                .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
                .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
                .layer(
                    TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
                        let request_id = request
                            .extensions()
                            .get::<RequestId>()
                            .map(|value| value.header_value().to_str().unwrap_or_default())
                            .unwrap_or_default();

                        tracing::info_span!(
                            "HTTP",
                            http.method = %request.method(),
                            http.url = %request.uri(),
                            request_id = %request_id,
                        )
                    }),
                ),
        )
        .layer(Extension(env))
}

fn hosts() -> Router {
    Router::new().route("/hosts", get(host::handler::list).post(host::handler::add))
}

fn vms() -> Router {
    Router::new().route("/vms", get(vm::handler::list))
}

pub struct ApiResponse<T> {
    data: T,
    code: StatusCode,
}

impl<T> IntoResponse for ApiResponse<T>
where
    T: Send + Sync + Serialize,
{
    fn into_response(self) -> Response {
        let mut response = response::Json(json!({
            "response": self.data,
        }))
        .into_response();

        *response.status_mut() = self.code;
        response
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        #[serde_with::serde_as]
        #[serde_with::skip_serializing_none]
        #[derive(serde::Serialize)]
        struct ErrorResponse<'a> {
            // Serialize the `Display` output as the error message
            #[serde_as(as = "DisplayFromStr")]
            message: &'a Error,

            errors: Option<&'a ValidationErrors>,
        }

        let errors = match &self {
            Error::InvalidEntity(errors) => Some(errors),
            _ => None,
        };

        tracing::error!("API error: {:?}", self);
        (
            self.status_code(),
            Json(ErrorResponse {
                message: &self,
                errors,
            }),
        )
            .into_response()
    }
}

impl Error {
    fn status_code(&self) -> StatusCode {
        use Error::*;

        match self {
            Sqlx(_) => StatusCode::INTERNAL_SERVER_ERROR,
            InvalidEntity(_) | UnprocessableEntity(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Conflict(_) => StatusCode::CONFLICT,
        }
    }
}
