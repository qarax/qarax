use super::*;
use crate::{
    App,
    model::audit_log::{self, AuditAction, AuditLog, AuditLogQuery, AuditResourceType},
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
    /// Maximum number of entries to return (default: 100, max: 1000)
    pub limit: Option<u16>,
}

#[utoipa::path(
    get,
    path = "/audit-logs",
    params(AuditLogListQuery),
    responses(
        (status = 200, description = "List audit log entries", body = Vec<AuditLog>),
        (status = 400, description = "Invalid query parameters"),
        (status = 500, description = "Internal server error")
    ),
    tag = "audit-logs"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(params): axum::extract::Query<AuditLogListQuery>,
) -> Result<ApiResponse<Vec<AuditLog>>> {
    let query = AuditLogQuery {
        resource_type: params.resource_type,
        resource_id: params.resource_id,
        action: params.action,
        limit: i64::from(params.limit.unwrap_or(100).min(1000)),
    };

    let logs = audit_log::list(env.pool(), query).await?;
    Ok(ApiResponse {
        data: logs,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/audit-logs/{audit_log_id}",
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
