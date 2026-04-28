use super::*;
use crate::{
    App,
    handlers::audit::{AuditEvent, AuditEventExt},
    model::{
        audit_log::{AuditAction, AuditResourceType},
        security_groups::{
            self, NewSecurityGroup, NewSecurityGroupRule, SecurityGroup, SecurityGroupRule,
        },
    },
    network_policy,
};
use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/security-groups",
    params(crate::handlers::NameQuery),
    responses(
        (status = 200, description = "List security groups", body = Vec<SecurityGroup>),
        (status = 500, description = "Internal server error")
    ),
    tag = "security-groups"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::NameQuery>,
) -> Result<ApiResponse<Vec<SecurityGroup>>> {
    let groups = security_groups::list(env.pool(), query.name.as_deref()).await?;
    Ok(ApiResponse {
        data: groups,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/security-groups/{security_group_id}",
    params(
        ("security_group_id" = uuid::Uuid, Path, description = "Security group unique identifier")
    ),
    responses(
        (status = 200, description = "Security group found", body = SecurityGroup),
        (status = 404, description = "Security group not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "security-groups"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(security_group_id): Path<Uuid>,
) -> Result<ApiResponse<SecurityGroup>> {
    let group = security_groups::get(env.pool(), security_group_id).await?;
    Ok(ApiResponse {
        data: group,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/security-groups",
    request_body = NewSecurityGroup,
    responses(
        (status = 201, description = "Security group created successfully", body = String),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "security-groups"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(new_group): Json<NewSecurityGroup>,
) -> Result<axum::response::Response> {
    let group_name = new_group.name.clone();
    let id = security_groups::create(env.pool(), new_group).await?;
    Ok(
        (StatusCode::CREATED, id.to_string()).with_audit_event(AuditEvent {
            action: AuditAction::Create,
            resource_type: AuditResourceType::SecurityGroup,
            resource_id: id,
            resource_name: Some(group_name),
            metadata: None,
        }),
    )
}

#[utoipa::path(
    delete,
    path = "/security-groups/{security_group_id}",
    params(
        ("security_group_id" = uuid::Uuid, Path, description = "Security group unique identifier")
    ),
    responses(
        (status = 204, description = "Security group deleted successfully"),
        (status = 404, description = "Security group not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "security-groups"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(security_group_id): Path<Uuid>,
) -> Result<axum::response::Response> {
    let group = security_groups::get(env.pool(), security_group_id).await?;
    let vm_ids = security_groups::list_vm_ids(env.pool(), security_group_id).await?;
    security_groups::delete(env.pool(), security_group_id).await?;
    for vm_id in vm_ids {
        network_policy::sync_vm_firewall(&env, vm_id).await?;
    }
    Ok(StatusCode::NO_CONTENT.with_audit_event(AuditEvent {
        action: AuditAction::Delete,
        resource_type: AuditResourceType::SecurityGroup,
        resource_id: security_group_id,
        resource_name: Some(group.name),
        metadata: None,
    }))
}

#[utoipa::path(
    get,
    path = "/security-groups/{security_group_id}/rules",
    params(
        ("security_group_id" = uuid::Uuid, Path, description = "Security group unique identifier")
    ),
    responses(
        (status = 200, description = "List security-group rules", body = Vec<SecurityGroupRule>),
        (status = 404, description = "Security group not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "security-groups"
)]
#[instrument(skip(env))]
pub async fn list_rules(
    Extension(env): Extension<App>,
    Path(security_group_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<SecurityGroupRule>>> {
    security_groups::get(env.pool(), security_group_id).await?;
    let rules = security_groups::list_rules(env.pool(), security_group_id).await?;
    Ok(ApiResponse {
        data: rules,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/security-groups/{security_group_id}/rules",
    params(
        ("security_group_id" = uuid::Uuid, Path, description = "Security group unique identifier")
    ),
    request_body = NewSecurityGroupRule,
    responses(
        (status = 201, description = "Security-group rule created successfully", body = String),
        (status = 404, description = "Security group not found"),
        (status = 422, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    tag = "security-groups"
)]
#[instrument(skip(env))]
pub async fn create_rule(
    Extension(env): Extension<App>,
    Path(security_group_id): Path<Uuid>,
    Json(rule): Json<NewSecurityGroupRule>,
) -> Result<(StatusCode, String)> {
    security_groups::get(env.pool(), security_group_id).await?;
    let id = security_groups::create_rule(env.pool(), security_group_id, rule).await?;
    network_policy::sync_security_group_members(&env, security_group_id).await?;
    Ok((StatusCode::CREATED, id.to_string()))
}

#[utoipa::path(
    delete,
    path = "/security-groups/{security_group_id}/rules/{rule_id}",
    params(
        ("security_group_id" = uuid::Uuid, Path, description = "Security group unique identifier"),
        ("rule_id" = uuid::Uuid, Path, description = "Security-group rule unique identifier")
    ),
    responses(
        (status = 204, description = "Security-group rule deleted successfully"),
        (status = 404, description = "Security group or rule not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "security-groups"
)]
#[instrument(skip(env))]
pub async fn delete_rule(
    Extension(env): Extension<App>,
    Path((security_group_id, rule_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode> {
    security_groups::get(env.pool(), security_group_id).await?;
    security_groups::delete_rule(env.pool(), security_group_id, rule_id).await?;
    network_policy::sync_security_group_members(&env, security_group_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
