use super::*;
use crate::{
    App,
    model::audit_log::{self, AuditAction, AuditLog, AuditLogQuery, AuditResourceType},
    pagination::{PageQuery, PageRequest, time_key},
};
use axum::{Extension, extract::Path};
use http::StatusCode;
use serde::Deserialize;
use tracing::instrument;
use utoipa::IntoParams;
use uuid::Uuid;

#[derive(Deserialize, IntoParams, Debug)]
pub struct AuditLogListQuery {
    /// Filter by resource type (e.g. "vm", "host")
    pub resource_type: Option<AuditResourceType>,
    /// Filter by resource UUID
    pub resource_id: Option<Uuid>,
    /// Filter by action (e.g. "create", "start", "delete")
    pub action: Option<AuditAction>,
}

#[utoipa::path(
    get,
    path = "/v1/audit-logs",
    params(AuditLogListQuery, PageQuery),
    responses(
        (status = 200, description = "Audit log entries, newest first", body = crate::pagination::AuditLogPage),
        (status = 400, description = "Invalid query parameters"),
        (status = 422, description = "Invalid limit or cursor"),
        (status = 500, description = "Internal server error")
    ),
    tag = "audit-logs"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(params): axum::extract::Query<AuditLogListQuery>,
    axum::extract::Query(page): axum::extract::Query<PageQuery>,
) -> Result<ApiResponse<crate::pagination::AuditLogPage>> {
    let page = PageRequest::try_from(page)?;
    let query = AuditLogQuery {
        resource_type: params.resource_type,
        resource_id: params.resource_id,
        action: params.action,
    };

    let logs = audit_log::list(env.pool(), query, &page).await?;
    Ok(ApiResponse {
        data: crate::pagination::AuditLogPage::new(logs, &page, |log| {
            (time_key(log.created_at), log.id)
        }),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/v1/audit-logs/{audit_log_id}",
    params(
        ("audit_log_id" = Uuid, Path, description = "Audit log entry ID")
    ),
    responses(
        (status = 200, description = "Get audit log entry", body = AuditLog),
        (status = 404, description = "Not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "audit-logs"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(audit_log_id): Path<Uuid>,
) -> Result<ApiResponse<AuditLog>> {
    let log = audit_log::get(env.pool(), audit_log_id).await?;
    Ok(ApiResponse {
        data: log,
        code: StatusCode::OK,
    })
}
