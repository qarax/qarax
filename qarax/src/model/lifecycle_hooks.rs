use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::rust::double_option;
use sqlx::{PgPool, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use super::vms::Vm;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "hook_scope")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HookScope {
    Global,
    Vm,
    Tag,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "hook_execution_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HookExecutionStatus {
    Pending,
    Delivered,
    Failed,
}

// ---------------------------------------------------------------------------
// Domain models
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, sqlx::FromRow)]
pub struct LifecycleHook {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    pub scope: HookScope,
    pub scope_value: Option<String>,
    #[sqlx(default)]
    pub events: Vec<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, sqlx::FromRow)]
pub struct HookExecution {
    pub id: Uuid,
    pub hook_id: Uuid,
    pub vm_id: Uuid,
    pub previous_status: String,
    pub new_status: String,
    pub status: HookExecutionStatus,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub next_retry_at: DateTime<Utc>,
    pub payload: serde_json::Value,
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug, ToSchema)]
pub struct NewLifecycleHook {
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    #[serde(default = "default_scope")]
    pub scope: HookScope,
    pub scope_value: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
}

fn default_scope() -> HookScope {
    HookScope::Global
}

#[derive(Deserialize, Debug, ToSchema)]
pub struct UpdateLifecycleHook {
    pub url: Option<String>,
    #[serde(default, with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub secret: Option<Option<String>>,
    pub scope: Option<HookScope>,
    #[serde(default, with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub scope_value: Option<Option<String>>,
    pub events: Option<Vec<String>>,
    pub active: Option<bool>,
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

pub async fn create(pool: &PgPool, hook: NewLifecycleHook) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query(
        r#"
INSERT INTO lifecycle_hooks (id, name, url, secret, scope, scope_value, events)
VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(&hook.name)
    .bind(&hook.url)
    .bind(&hook.secret)
    .bind(&hook.scope)
    .bind(&hook.scope_value)
    .bind(&hook.events)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn get(pool: &PgPool, hook_id: Uuid) -> Result<LifecycleHook, sqlx::Error> {
    let hook = sqlx::query_as::<_, LifecycleHook>(
        r#"
SELECT id, name, url, secret, scope, scope_value, events, active,
       created_at, updated_at
FROM lifecycle_hooks
WHERE id = $1
        "#,
    )
    .bind(hook_id)
    .fetch_one(pool)
    .await?;

    Ok(hook)
}

pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
) -> Result<Vec<LifecycleHook>, sqlx::Error> {
    let hooks = sqlx::query_as::<_, LifecycleHook>(
        r#"
SELECT id, name, url, secret, scope, scope_value, events, active,
       created_at, updated_at
FROM lifecycle_hooks
WHERE ($1::text IS NULL OR name = $1)
ORDER BY created_at
        "#,
    )
    .bind(name_filter)
    .fetch_all(pool)
    .await?;

    Ok(hooks)
}

pub async fn update(
    pool: &PgPool,
    hook_id: Uuid,
    req: UpdateLifecycleHook,
) -> Result<LifecycleHook, sqlx::Error> {
    let url_present = req.url.is_some();
    let secret_present = req.secret.is_some();
    let secret = req.secret.flatten();
    let scope_present = req.scope.is_some();
    let scope_value_present = req.scope_value.is_some();
    let scope_value = req.scope_value.flatten();
    let events_present = req.events.is_some();
    let active_present = req.active.is_some();

    sqlx::query(
        r#"
UPDATE lifecycle_hooks
SET url         = CASE WHEN $2  THEN $3  ELSE url END,
    secret      = CASE WHEN $4  THEN $5  ELSE secret END,
    scope       = CASE WHEN $6  THEN $7  ELSE scope END,
    scope_value = CASE WHEN $8  THEN $9  ELSE scope_value END,
    events      = CASE WHEN $10 THEN $11 ELSE events END,
    active      = CASE WHEN $12 THEN $13 ELSE active END,
    updated_at  = NOW()
WHERE id = $1
        "#,
    )
    .bind(hook_id)
    .bind(url_present)
    .bind(&req.url)
    .bind(secret_present)
    .bind(secret)
    .bind(scope_present)
    .bind(&req.scope)
    .bind(scope_value_present)
    .bind(scope_value)
    .bind(events_present)
    .bind(&req.events)
    .bind(active_present)
    .bind(req.active)
    .execute(pool)
    .await?;

    get(pool, hook_id).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::UpdateLifecycleHook;

    #[test]
    fn update_request_distinguishes_missing_and_null_nullable_fields() {
        let omitted: UpdateLifecycleHook = serde_json::from_value(json!({})).unwrap();
        assert_eq!(omitted.secret, None);
        assert_eq!(omitted.scope_value, None);

        let nulls: UpdateLifecycleHook =
            serde_json::from_value(json!({"secret": null, "scope_value": null})).unwrap();
        assert_eq!(nulls.secret, Some(None));
        assert_eq!(nulls.scope_value, Some(None));

        let values: UpdateLifecycleHook = serde_json::from_value(json!({
            "secret": "new-secret",
            "scope_value": "tag-a"
        }))
        .unwrap();
        assert_eq!(values.secret, Some(Some("new-secret".to_string())));
        assert_eq!(values.scope_value, Some(Some("tag-a".to_string())));
    }
}

pub async fn delete(pool: &PgPool, hook_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM lifecycle_hooks WHERE id = $1")
        .bind(hook_id)
        .execute(pool)
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Hook execution queue
// ---------------------------------------------------------------------------

/// Build the webhook payload JSON for a VM state transition.
fn build_payload(vm: &Vm, previous_status: &str, new_status: &str) -> serde_json::Value {
    serde_json::json!({
        "event": "vm.status_changed",
        "timestamp": Utc::now().to_rfc3339(),
        "vm_id": vm.id,
        "vm_name": vm.name,
        "previous_status": previous_status,
        "new_status": new_status,
        "host_id": vm.host_id,
        "tags": vm.tags,
    })
}

/// Find all active hooks matching this VM + transition and enqueue executions.
pub async fn enqueue_matching(
    pool: &PgPool,
    vm: &Vm,
    previous_status: &str,
    new_status: &str,
) -> Result<(), sqlx::Error> {
    // Find hooks whose scope matches this VM and whose events filter matches.
    let hooks = sqlx::query_as::<_, LifecycleHook>(
        r#"
SELECT id, name, url, secret, scope, scope_value, events, active,
       created_at, updated_at
FROM lifecycle_hooks
WHERE active = TRUE
  AND (
      scope = 'GLOBAL'
      OR (scope = 'VM'  AND scope_value = $1::text)
      OR (scope = 'TAG' AND scope_value = ANY($2))
  )
  AND (
      events = ARRAY[]::TEXT[]
      OR $3 = ANY(events)
  )
        "#,
    )
    .bind(vm.id.to_string())
    .bind(&vm.tags)
    .bind(new_status)
    .fetch_all(pool)
    .await?;

    if hooks.is_empty() {
        return Ok(());
    }

    let payload = build_payload(vm, previous_status, new_status);

    for hook in hooks {
        sqlx::query(
            r#"
INSERT INTO hook_executions (hook_id, vm_id, previous_status, new_status, payload)
VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(hook.id)
        .bind(vm.id)
        .bind(previous_status)
        .bind(new_status)
        .bind(&payload)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Fetch pending executions ready for delivery.
pub async fn fetch_pending_executions(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<HookExecution>, sqlx::Error> {
    let executions = sqlx::query_as::<_, HookExecution>(
        r#"
SELECT id, hook_id, vm_id, previous_status, new_status, status,
       attempt_count, max_attempts, next_retry_at, payload,
       response_status, response_body, last_error,
       created_at, delivered_at
FROM hook_executions
WHERE status = 'PENDING'
  AND next_retry_at <= NOW()
ORDER BY next_retry_at
LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(executions)
}

pub async fn mark_delivered(
    pool: &PgPool,
    execution_id: Uuid,
    response_status: i32,
    response_body: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE hook_executions
SET status          = 'DELIVERED',
    response_status = $2,
    response_body   = $3,
    delivered_at    = NOW()
WHERE id = $1
        "#,
    )
    .bind(execution_id)
    .bind(response_status)
    .bind(response_body)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_retry(
    pool: &PgPool,
    execution_id: Uuid,
    error: &str,
    next_retry_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE hook_executions
SET attempt_count = attempt_count + 1,
    last_error    = $2,
    next_retry_at = $3
WHERE id = $1
        "#,
    )
    .bind(execution_id)
    .bind(error)
    .bind(next_retry_at)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_failed(
    pool: &PgPool,
    execution_id: Uuid,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE hook_executions
SET status     = 'FAILED',
    last_error = $2,
    attempt_count = attempt_count + 1
WHERE id = $1
        "#,
    )
    .bind(execution_id)
    .bind(error)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_executions(
    pool: &PgPool,
    hook_id: Uuid,
) -> Result<Vec<HookExecution>, sqlx::Error> {
    let executions = sqlx::query_as::<_, HookExecution>(
        r#"
SELECT id, hook_id, vm_id, previous_status, new_status, status,
       attempt_count, max_attempts, next_retry_at, payload,
       response_status, response_body, last_error,
       created_at, delivered_at
FROM hook_executions
WHERE hook_id = $1
ORDER BY created_at DESC
        "#,
    )
    .bind(hook_id)
    .fetch_all(pool)
    .await?;

    Ok(executions)
}
