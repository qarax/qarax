use crate::{App, errors::Error};
use axum::{
    Extension, Json, Router,
    body::Body,
    response::{self, IntoResponse, Response},
    routing::{get, post},
};
use http::{Request, StatusCode, header::HeaderName};
use serde::Serialize;
use serde_with::DisplayFromStr;
use tower::ServiceBuilder;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::TraceLayer,
};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use validator::ValidationErrors;

mod host;
mod vm;

pub type Result<T, E = Error> = ::std::result::Result<T, E>;

#[derive(OpenApi)]
#[openapi(
    paths(
        host::handler::list,
        host::handler::add,
        vm::handler::list,
        vm::handler::get,
        vm::handler::create,
        vm::handler::start,
        vm::handler::stop,
        vm::handler::pause,
        vm::handler::resume,
        vm::handler::delete,
    ),
    components(
        schemas(
            crate::model::hosts::Host,
            crate::model::hosts::NewHost,
            crate::model::hosts::HostStatus,
            crate::model::vms::Vm,
            crate::model::vms::NewVm,
            crate::model::vms::VmStatus,
            crate::model::vms::Hypervisor,
        )
    ),
    tags(
        (name = "hosts", description = "Host management endpoints"),
        (name = "vms", description = "Virtual machine management endpoints")
    ),
    info(
        title = "Qarax API",
        version = "0.1.0",
        description = "REST API for managing virtual machines and hypervisor hosts"
    )
)]
pub struct ApiDoc;

pub fn app(env: App) -> Router {
    let x_request_id = HeaderName::from_static("x-request-id");
    Router::new()
        .route("/", get(|| async { "hello" }))
        .merge(hosts())
        .merge(vms())
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
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
    Router::new()
        .route("/vms", get(vm::handler::list).post(vm::handler::create))
        .route(
            "/vms/:vm_id",
            get(vm::handler::get).delete(vm::handler::delete),
        )
        .route("/vms/:vm_id/start", post(vm::handler::start))
        .route("/vms/:vm_id/stop", post(vm::handler::stop))
        .route("/vms/:vm_id/pause", post(vm::handler::pause))
        .route("/vms/:vm_id/resume", post(vm::handler::resume))
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
        let mut response = response::Json(self.data).into_response();

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
            Sqlx(_) | InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            InvalidEntity(_) | UnprocessableEntity(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Conflict(_) => StatusCode::CONFLICT,
        }
    }
}
