use serde::{Deserialize, Serialize};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RateLimitGroup {
    pub id: Uuid,
    pub name: String,
    pub vm_id: Option<Uuid>,
    pub config: serde_json::Value, // RateLimiterConfig as JSON
}

#[derive(sqlx::FromRow)]
pub struct RateLimitGroupRow {
    pub id: Uuid,
    pub name: String,
    pub vm_id: Option<Uuid>,
    pub config: Json<serde_json::Value>,
}

impl From<RateLimitGroupRow> for RateLimitGroup {
    fn from(row: RateLimitGroupRow) -> Self {
        RateLimitGroup {
            id: row.id,
            name: row.name,
            vm_id: row.vm_id,
            config: row.config.0,
        }
    }
}

pub async fn list_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<Vec<RateLimitGroup>, sqlx::Error> {
    let groups: Vec<RateLimitGroupRow> = sqlx::query_as!(
        RateLimitGroupRow,
        r#"
SELECT id,
        name,
        vm_id as "vm_id?",
        config as "config: _"
FROM rate_limit_groups
WHERE vm_id = $1
ORDER BY name
        "#,
        vm_id
    )
    .fetch_all(pool)
    .await?;

    Ok(groups.into_iter().map(|g| g.into()).collect())
}

pub async fn get(pool: &PgPool, group_id: Uuid) -> Result<RateLimitGroup, sqlx::Error> {
    let group: RateLimitGroupRow = sqlx::query_as!(
        RateLimitGroupRow,
        r#"
SELECT id,
        name,
        vm_id as "vm_id?",
        config as "config: _"
FROM rate_limit_groups
WHERE id = $1
        "#,
        group_id
    )
    .fetch_one(pool)
    .await?;

    Ok(group.into())
}

pub async fn get_by_name(
    pool: &PgPool,
    vm_id: Uuid,
    name: &str,
) -> Result<Option<RateLimitGroup>, sqlx::Error> {
    let group: Option<RateLimitGroupRow> = sqlx::query_as!(
        RateLimitGroupRow,
        r#"
SELECT id,
        name,
        vm_id as "vm_id?",
        config as "config: _"
FROM rate_limit_groups
WHERE vm_id = $1 AND name = $2
        "#,
        vm_id,
        name
    )
    .fetch_optional(pool)
    .await?;

    Ok(group.map(|g| g.into()))
}
