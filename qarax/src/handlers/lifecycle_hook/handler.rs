use super::*;
use crate::{
    App,
    model::lifecycle_hooks::{
        self, HookExecution, LifecycleHook, NewLifecycleHook, UpdateLifecycleHook,
    },
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/hooks",
    params(crate::handlers::NameQuery),
    responses(
        (status = 200, description = "List all lifecycle hooks", body = Vec<LifecycleHook>),
        (status = 500, description = "Internal server error")
    ),
    tag = "hooks"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::NameQuery>,
) -> Result<ApiResponse<Vec<LifecycleHook>>> {
    let hooks = lifecycle_hooks::list(env.pool(), query.name.as_deref()).await?;
    Ok(ApiResponse {
        data: hooks,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/hooks/{hook_id}",
    params(
        ("hook_id" = uuid::Uuid, Path, description = "Lifecycle hook unique identifier")
    ),
    responses(
        (status = 200, description = "Lifecycle hook found", body = LifecycleHook),
        (status = 404, description = "Lifecycle hook not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hooks"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(hook_id): Path<Uuid>,
) -> Result<ApiResponse<LifecycleHook>> {
    let hook = lifecycle_hooks::get(env.pool(), hook_id).await?;
    Ok(ApiResponse {
        data: hook,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/hooks",
    request_body = NewLifecycleHook,
    responses(
        (status = 201, description = "Lifecycle hook created successfully", body = String),
        (status = 409, description = "Hook with this name already exists"),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hooks"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(new_hook): Json<NewLifecycleHook>,
) -> Result<(StatusCode, String)> {
    let id = lifecycle_hooks::create(env.pool(), new_hook).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    patch,
    path = "/hooks/{hook_id}",
    params(
        ("hook_id" = uuid::Uuid, Path, description = "Lifecycle hook unique identifier")
    ),
    request_body = UpdateLifecycleHook,
    responses(
        (status = 200, description = "Lifecycle hook updated", body = LifecycleHook),
        (status = 404, description = "Lifecycle hook not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hooks"
)]
#[instrument(skip(env))]
pub async fn update(
    Extension(env): Extension<App>,
    Path(hook_id): Path<Uuid>,
    Json(update): Json<UpdateLifecycleHook>,
) -> Result<ApiResponse<LifecycleHook>> {
    let hook = lifecycle_hooks::update(env.pool(), hook_id, update).await?;
    Ok(ApiResponse {
        data: hook,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    delete,
    path = "/hooks/{hook_id}",
    params(
        ("hook_id" = uuid::Uuid, Path, description = "Lifecycle hook unique identifier")
    ),
    responses(
        (status = 204, description = "Lifecycle hook deleted successfully"),
        (status = 404, description = "Lifecycle hook not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hooks"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(hook_id): Path<Uuid>,
) -> Result<StatusCode> {
    lifecycle_hooks::delete(env.pool(), hook_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/hooks/{hook_id}/executions",
    params(
        ("hook_id" = uuid::Uuid, Path, description = "Lifecycle hook unique identifier")
    ),
    responses(
        (status = 200, description = "List hook executions", body = Vec<HookExecution>),
        (status = 404, description = "Lifecycle hook not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "hooks"
)]
#[instrument(skip(env))]
pub async fn list_executions(
    Extension(env): Extension<App>,
    Path(hook_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<HookExecution>>> {
    // Verify the hook exists
    lifecycle_hooks::get(env.pool(), hook_id).await?;

    let executions = lifecycle_hooks::list_executions(env.pool(), hook_id).await?;
    Ok(ApiResponse {
        data: executions,
        code: StatusCode::OK,
    })
}
