use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, Display, EnumString, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AuditAction {
    Create,
    Update,
    Delete,
    Start,
    Stop,
    ForceStop,
    Pause,
    Resume,
    Deploy,
    Init,
    Migrate,
    Restore,
    AttachDisk,
    RemoveDisk,
    AddNic,
    RemoveNic,
    Resize,
    Commit,
    CreateSnapshot,
    RestoreSnapshot,
    CreateTemplate,
    NodeUpgrade,
}

#[derive(Serialize, Deserialize, Debug, Clone, Display, EnumString, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AuditResourceType {
    Vm,
    Host,
    StoragePool,
    StorageObject,
    Network,
    BootSource,
    VmTemplate,
    InstanceType,
    LifecycleHook,
    Transfer,
    Sandbox,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct AuditLog {
    pub id: Uuid,
    pub action: AuditAction,
    pub resource_type: AuditResourceType,
    pub resource_id: Uuid,
    pub resource_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Internal row type for sqlx deserialization.
#[derive(sqlx::FromRow)]
struct AuditLogRow {
    pub id: Uuid,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Uuid,
    pub resource_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<AuditLogRow> for AuditLog {
    type Error = crate::errors::Error;

    fn try_from(row: AuditLogRow) -> Result<Self, Self::Error> {
        let AuditLogRow {
            id,
            action,
            resource_type,
            resource_id,
            resource_name,
            metadata,
            created_at,
        } = row;
        let action = action.parse().map_err(|error| {
            tracing::error!(%error, action = action.as_str(), "invalid audit log action stored");
            crate::errors::Error::InternalServerError
        })?;
        let resource_type = resource_type.parse().map_err(|error| {
            tracing::error!(
                %error,
                resource_type = resource_type.as_str(),
                "invalid audit log resource type stored"
            );
            crate::errors::Error::InternalServerError
        })?;

        Ok(AuditLog {
            id,
            action,
            resource_type,
            resource_id,
            resource_name,
            metadata,
            created_at,
        })
    }
}

pub struct NewAuditLog {
    pub action: AuditAction,
    pub resource_type: AuditResourceType,
    pub resource_id: Uuid,
    pub resource_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn record(pool: &PgPool, entry: NewAuditLog) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
INSERT INTO audit_logs (action, resource_type, resource_id, resource_name, metadata)
VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(entry.action.to_string())
    .bind(entry.resource_type.to_string())
    .bind(entry.resource_id)
    .bind(entry.resource_name)
    .bind(entry.metadata.map(sqlx::types::Json))
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn record_best_effort(pool: &PgPool, entry: NewAuditLog) {
    if let Err(error) = record(pool, entry).await {
        tracing::warn!(%error, "Failed to record audit log");
    }
}

pub struct AuditLogQuery {
    pub resource_type: Option<AuditResourceType>,
    pub resource_id: Option<Uuid>,
    pub action: Option<AuditAction>,
    pub limit: i64,
}

impl Default for AuditLogQuery {
    fn default() -> Self {
        AuditLogQuery {
            resource_type: None,
            resource_id: None,
            action: None,
            limit: 100,
        }
    }
}

pub async fn list(
    pool: &PgPool,
    query: AuditLogQuery,
) -> Result<Vec<AuditLog>, crate::errors::Error> {
    let mut qb = QueryBuilder::<Postgres>::new(
        "SELECT id, action, resource_type, resource_id, resource_name, metadata, created_at FROM audit_logs WHERE 1=1 ",
    );

    if let Some(ref rt) = query.resource_type {
        qb.push("AND resource_type = ");
        qb.push_bind(rt.to_string());
        qb.push(' ');
    }

    if let Some(rid) = query.resource_id {
        qb.push("AND resource_id = ");
        qb.push_bind(rid);
        qb.push(' ');
    }

    if let Some(ref action) = query.action {
        qb.push("AND action = ");
        qb.push_bind(action.to_string());
        qb.push(' ');
    }

    qb.push("ORDER BY created_at DESC LIMIT ");
    qb.push_bind(query.limit);

    let rows = qb.build_query_as::<AuditLogRow>().fetch_all(pool).await?;

    rows.into_iter().map(AuditLog::try_from).collect()
}

pub async fn get(pool: &PgPool, id: Uuid) -> Result<AuditLog, crate::errors::Error> {
    let row = sqlx::query_as::<_, AuditLogRow>(
        r#"
SELECT id, action, resource_type, resource_id, resource_name, metadata, created_at
FROM audit_logs
WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    AuditLog::try_from(row)
}
