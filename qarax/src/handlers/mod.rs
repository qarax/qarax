use crate::{App, errors::Error};
use axum::{
    Extension, Json, Router,
    body::Body,
    response::{self, IntoResponse, Response},
    routing::{get, patch, post},
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

mod boot_source;
mod host;
mod instance_type;
mod job;
mod lifecycle_hook;
mod network;
mod storage_object;
mod storage_pool;
mod transfer;
mod vm;
mod vm_template;

pub type Result<T, E = Error> = ::std::result::Result<T, E>;

#[derive(serde::Deserialize, utoipa::IntoParams, Debug)]
pub struct NameQuery {
    /// Optional name filter for list queries
    pub name: Option<String>,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        host::handler::list,
        host::handler::add,
        host::handler::update,
        host::handler::deploy,
        host::handler::init,
        host::handler::list_gpus,
        instance_type::handler::list,
        instance_type::handler::get,
        instance_type::handler::create,
        instance_type::handler::delete,
        vm::handler::list,
        vm::handler::get,
        vm::handler::create,
        vm::handler::create_template_from_vm,
        vm::handler::start,
        vm::handler::stop,
        vm::handler::force_stop,
        vm::handler::pause,
        vm::handler::resume,
        vm::handler::list_snapshots,
        vm::handler::create_snapshot,
        vm::handler::restore,
        vm::handler::migrate,
        vm::handler::delete,
        vm::handler::metrics,
        vm::handler::console_log,
        vm::handler::console_attach,
        vm::handler::attach_disk,
        vm::handler::remove_disk,
        vm::handler::add_nic,
        vm::handler::remove_nic,
        vm::handler::resize_vm,
        storage_object::handler::list,
        storage_object::handler::get,
        storage_object::handler::create,
        storage_object::handler::delete,
        storage_pool::handler::list,
        storage_pool::handler::get,
        storage_pool::handler::create,
        storage_pool::handler::delete,
        storage_pool::handler::attach_host,
        storage_pool::handler::detach_host,
        storage_pool::handler::import_to_pool,
        boot_source::handler::list,
        boot_source::handler::get,
        boot_source::handler::create,
        boot_source::handler::delete,
        vm_template::handler::list,
        vm_template::handler::get,
        vm_template::handler::create,
        vm_template::handler::delete,
        transfer::handler::create,
        transfer::handler::list,
        transfer::handler::get,
        job::handler::get,
        network::handler::list,
        network::handler::get,
        network::handler::create,
        network::handler::delete,
        network::handler::attach_host,
        network::handler::detach_host,
        network::handler::list_ips,
        lifecycle_hook::handler::list,
        lifecycle_hook::handler::get,
        lifecycle_hook::handler::create,
        lifecycle_hook::handler::update,
        lifecycle_hook::handler::delete,
        lifecycle_hook::handler::list_executions,
    ),
    components(
        schemas(
            crate::model::hosts::Host,
            crate::model::hosts::NewHost,
            crate::model::hosts::UpdateHostRequest,
            crate::model::hosts::DeployHostRequest,
            crate::model::hosts::HostStatus,
            crate::model::host_gpus::HostGpu,
            crate::model::host_gpus::AcceleratorConfig,
            crate::model::instance_types::InstanceType,
            crate::model::instance_types::NewInstanceType,
            crate::model::vms::Vm,
            crate::model::vms::NewVm,
            crate::model::vms::NewVmNetwork,
            crate::model::vms::VmStatus,
            crate::model::vms::Hypervisor,
            crate::model::vm_templates::CreateVmTemplateFromVmRequest,
            crate::model::storage_objects::StorageObject,
            crate::model::storage_objects::NewStorageObject,
            crate::model::storage_objects::StorageObjectType,
            crate::model::storage_pools::StoragePool,
            crate::model::storage_pools::NewStoragePool,
            crate::model::storage_pools::StoragePoolType,
            crate::model::storage_pools::StoragePoolStatus,
            crate::handlers::storage_pool::handler::AttachPoolHostRequest,
            crate::model::boot_sources::BootSource,
            crate::model::boot_sources::NewBootSource,
            crate::model::vm_templates::VmTemplate,
            crate::model::vm_templates::NewVmTemplate,
            crate::model::network_interfaces::NetworkInterface,
            crate::model::network_interfaces::RateLimiterConfig,
            crate::model::network_interfaces::TokenBucket,
            crate::model::network_interfaces::InterfaceType,
            crate::model::network_interfaces::VhostMode,
            crate::handlers::vm::handler::VmMetrics,
            crate::model::transfers::Transfer,
            crate::model::transfers::NewTransfer,
            crate::model::transfers::TransferType,
            crate::model::transfers::TransferStatus,
            crate::model::vm_filesystems::VmFilesystem,
            crate::model::vm_filesystems::NewVmFilesystem,
            crate::model::vm_disks::VmDisk,
            crate::model::jobs::Job,
            crate::model::jobs::JobStatus,
            crate::model::jobs::JobType,
            crate::model::snapshots::Snapshot,
            crate::model::snapshots::SnapshotStatus,
            crate::handlers::vm::handler::CreateVmResponse,
            crate::handlers::vm::handler::VmStartResponse,
            crate::handlers::vm::handler::AttachDiskRequest,
            crate::handlers::vm::handler::RestoreRequest,
            crate::handlers::vm::handler::VmMigrateRequest,
            crate::handlers::vm::handler::VmMigrateResponse,
            crate::handlers::vm::handler::VmResizeRequest,
            crate::handlers::storage_pool::handler::ImportToPoolRequest,
            crate::handlers::storage_pool::handler::ImportToPoolResponse,
            crate::model::networks::Network,
            crate::model::networks::NewNetwork,
            crate::model::networks::NetworkStatus,
            crate::model::networks::IpAllocation,
            crate::handlers::network::handler::AttachHostRequest,
            crate::model::lifecycle_hooks::LifecycleHook,
            crate::model::lifecycle_hooks::NewLifecycleHook,
            crate::model::lifecycle_hooks::UpdateLifecycleHook,
            crate::model::lifecycle_hooks::HookExecution,
            crate::model::lifecycle_hooks::HookScope,
            crate::model::lifecycle_hooks::HookExecutionStatus,
        )
    ),
    tags(
        (name = "hosts", description = "Host management endpoints"),
        (name = "instance-types", description = "Instance type management endpoints"),
        (name = "vms", description = "Virtual machine management endpoints"),
        (name = "vm-templates", description = "VM template management endpoints"),
        (name = "storage-objects", description = "Storage object management endpoints"),
        (name = "storage-pools", description = "Storage pool management endpoints"),
        (name = "boot-sources", description = "Boot source management endpoints"),
        (name = "transfers", description = "File transfer management endpoints"),
        (name = "jobs", description = "Async job management endpoints"),
        (name = "networks", description = "Network management endpoints"),
        (name = "hooks", description = "Lifecycle hook management endpoints")
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
        .merge(instance_types())
        .merge(vms())
        .merge(vm_templates())
        .merge(storage_objects())
        .merge(storage_pools())
        .merge(boot_sources())
        .merge(transfers())
        .merge(jobs())
        .merge(networks())
        .merge(hooks())
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
    Router::new()
        .route("/hosts", get(host::handler::list).post(host::handler::add))
        .route("/hosts/{host_id}", patch(host::handler::update))
        .route("/hosts/{host_id}/deploy", post(host::handler::deploy))
        .route("/hosts/{host_id}/init", post(host::handler::init))
        .route("/hosts/{host_id}/gpus", get(host::handler::list_gpus))
}

fn vms() -> Router {
    Router::new()
        .route("/vms", get(vm::handler::list).post(vm::handler::create))
        .route(
            "/vms/{vm_id}",
            get(vm::handler::get).delete(vm::handler::delete),
        )
        .route("/vms/{vm_id}/start", post(vm::handler::start))
        .route(
            "/vms/{vm_id}/template",
            post(vm::handler::create_template_from_vm),
        )
        .route("/vms/{vm_id}/stop", post(vm::handler::stop))
        .route("/vms/{vm_id}/force-stop", post(vm::handler::force_stop))
        .route("/vms/{vm_id}/pause", post(vm::handler::pause))
        .route("/vms/{vm_id}/resume", post(vm::handler::resume))
        .route("/vms/{vm_id}/metrics", get(vm::handler::metrics))
        .route("/vms/{vm_id}/console", get(vm::handler::console_log))
        .route(
            "/vms/{vm_id}/console/attach",
            get(vm::handler::console_attach),
        )
        .route("/vms/{vm_id}/disks", post(vm::handler::attach_disk))
        .route(
            "/vms/{vm_id}/disks/{device_id}",
            axum::routing::delete(vm::handler::remove_disk),
        )
        .route("/vms/{vm_id}/nics", post(vm::handler::add_nic))
        .route(
            "/vms/{vm_id}/nics/{device_id}",
            axum::routing::delete(vm::handler::remove_nic),
        )
        .route(
            "/vms/{vm_id}/snapshots",
            get(vm::handler::list_snapshots).post(vm::handler::create_snapshot),
        )
        .route("/vms/{vm_id}/restore", post(vm::handler::restore))
        .route("/vms/{vm_id}/migrate", post(vm::handler::migrate))
        .route(
            "/vms/{vm_id}/resize",
            axum::routing::put(vm::handler::resize_vm),
        )
}

fn instance_types() -> Router {
    Router::new()
        .route(
            "/instance-types",
            get(instance_type::handler::list).post(instance_type::handler::create),
        )
        .route(
            "/instance-types/{instance_type_id}",
            get(instance_type::handler::get).delete(instance_type::handler::delete),
        )
}

fn vm_templates() -> Router {
    Router::new()
        .route(
            "/vm-templates",
            get(vm_template::handler::list).post(vm_template::handler::create),
        )
        .route(
            "/vm-templates/{vm_template_id}",
            get(vm_template::handler::get).delete(vm_template::handler::delete),
        )
}

fn storage_objects() -> Router {
    Router::new()
        .route(
            "/storage-objects",
            get(storage_object::handler::list).post(storage_object::handler::create),
        )
        .route(
            "/storage-objects/{object_id}",
            get(storage_object::handler::get).delete(storage_object::handler::delete),
        )
}

fn storage_pools() -> Router {
    Router::new()
        .route(
            "/storage-pools",
            get(storage_pool::handler::list).post(storage_pool::handler::create),
        )
        .route(
            "/storage-pools/{pool_id}",
            get(storage_pool::handler::get).delete(storage_pool::handler::delete),
        )
        .route(
            "/storage-pools/{pool_id}/hosts",
            post(storage_pool::handler::attach_host),
        )
        .route(
            "/storage-pools/{pool_id}/hosts/{host_id}",
            axum::routing::delete(storage_pool::handler::detach_host),
        )
        .route(
            "/storage-pools/{pool_id}/import",
            post(storage_pool::handler::import_to_pool),
        )
}

fn transfers() -> Router {
    Router::new()
        .route(
            "/storage-pools/{pool_id}/transfers",
            get(transfer::handler::list).post(transfer::handler::create),
        )
        .route(
            "/storage-pools/{pool_id}/transfers/{transfer_id}",
            get(transfer::handler::get),
        )
}

fn jobs() -> Router {
    Router::new().route("/jobs/{job_id}", get(job::handler::get))
}

fn networks() -> Router {
    Router::new()
        .route(
            "/networks",
            get(network::handler::list).post(network::handler::create),
        )
        .route(
            "/networks/{network_id}",
            get(network::handler::get).delete(network::handler::delete),
        )
        .route(
            "/networks/{network_id}/hosts",
            post(network::handler::attach_host),
        )
        .route(
            "/networks/{network_id}/hosts/{host_id}",
            axum::routing::delete(network::handler::detach_host),
        )
        .route(
            "/networks/{network_id}/ips",
            get(network::handler::list_ips),
        )
}

fn hooks() -> Router {
    Router::new()
        .route(
            "/hooks",
            get(lifecycle_hook::handler::list).post(lifecycle_hook::handler::create),
        )
        .route(
            "/hooks/{hook_id}",
            get(lifecycle_hook::handler::get)
                .patch(lifecycle_hook::handler::update)
                .delete(lifecycle_hook::handler::delete),
        )
        .route(
            "/hooks/{hook_id}/executions",
            get(lifecycle_hook::handler::list_executions),
        )
}

fn boot_sources() -> Router {
    Router::new()
        .route(
            "/boot-sources",
            get(boot_source::handler::list).post(boot_source::handler::create),
        )
        .route(
            "/boot-sources/{boot_source_id}",
            get(boot_source::handler::get).delete(boot_source::handler::delete),
        )
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
            NotFound => StatusCode::NOT_FOUND,
        }
    }
}
