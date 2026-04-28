use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

pub type PgTransaction<'a> = Transaction<'a, Postgres>;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct SecurityGroup {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

#[derive(sqlx::FromRow)]
struct SecurityGroupRow {
    id: Uuid,
    name: String,
    description: Option<String>,
}

impl From<SecurityGroupRow> for SecurityGroup {
    fn from(row: SecurityGroupRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
        }
    }
}

#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "security_group_direction")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SecurityGroupDirection {
    Ingress,
    Egress,
}

#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "security_group_protocol")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SecurityGroupProtocol {
    Any,
    Tcp,
    Udp,
    Icmp,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct SecurityGroupRule {
    pub id: Uuid,
    pub security_group_id: Uuid,
    pub direction: SecurityGroupDirection,
    pub protocol: SecurityGroupProtocol,
    pub cidr: Option<String>,
    pub port_start: Option<i32>,
    pub port_end: Option<i32>,
    pub description: Option<String>,
}

#[derive(sqlx::FromRow)]
struct SecurityGroupRuleRow {
    id: Uuid,
    security_group_id: Uuid,
    direction: SecurityGroupDirection,
    protocol: SecurityGroupProtocol,
    cidr: Option<String>,
    port_start: Option<i32>,
    port_end: Option<i32>,
    description: Option<String>,
}

impl From<SecurityGroupRuleRow> for SecurityGroupRule {
    fn from(row: SecurityGroupRuleRow) -> Self {
        Self {
            id: row.id,
            security_group_id: row.security_group_id,
            direction: row.direction,
            protocol: row.protocol,
            cidr: row.cidr,
            port_start: row.port_start,
            port_end: row.port_end,
            description: row.description,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewSecurityGroup {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewSecurityGroupRule {
    pub direction: SecurityGroupDirection,
    pub protocol: SecurityGroupProtocol,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cidr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_start: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_end: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
) -> Result<Vec<SecurityGroup>, sqlx::Error> {
    let rows: Vec<SecurityGroupRow> = sqlx::query_as(
        r#"
SELECT id, name, description
FROM security_groups
WHERE ($1::text IS NULL OR name = $1)
ORDER BY name
        "#,
    )
    .bind(name_filter)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &PgPool, security_group_id: Uuid) -> Result<SecurityGroup, sqlx::Error> {
    let row: SecurityGroupRow = sqlx::query_as(
        r#"
SELECT id, name, description
FROM security_groups
WHERE id = $1
        "#,
    )
    .bind(security_group_id)
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

pub async fn create(pool: &PgPool, new_group: NewSecurityGroup) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query(
        r#"
INSERT INTO security_groups (id, name, description)
VALUES ($1, $2, $3)
        "#,
    )
    .bind(id)
    .bind(new_group.name)
    .bind(new_group.description)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, security_group_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM security_groups WHERE id = $1")
        .bind(security_group_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_rules(
    pool: &PgPool,
    security_group_id: Uuid,
) -> Result<Vec<SecurityGroupRule>, sqlx::Error> {
    let rows: Vec<SecurityGroupRuleRow> = sqlx::query_as(
        r#"
SELECT id,
       security_group_id,
       direction,
       protocol,
       cidr::text,
       port_start,
       port_end,
       description
FROM security_group_rules
WHERE security_group_id = $1
ORDER BY direction, protocol, cidr NULLS FIRST, port_start NULLS FIRST, id
        "#,
    )
    .bind(security_group_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn create_rule(
    pool: &PgPool,
    security_group_id: Uuid,
    rule: NewSecurityGroupRule,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();

    sqlx::query(
        r#"
INSERT INTO security_group_rules (
    id, security_group_id, direction, protocol, cidr, port_start, port_end, description
)
VALUES ($1, $2, $3, $4, $5::cidr, $6, $7, $8)
        "#,
    )
    .bind(id)
    .bind(security_group_id)
    .bind(rule.direction)
    .bind(rule.protocol)
    .bind(rule.cidr)
    .bind(rule.port_start)
    .bind(rule.port_end)
    .bind(rule.description)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn delete_rule(
    pool: &PgPool,
    security_group_id: Uuid,
    rule_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM security_group_rules WHERE security_group_id = $1 AND id = $2")
        .bind(security_group_id)
        .bind(rule_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<Vec<SecurityGroup>, sqlx::Error> {
    let rows: Vec<SecurityGroupRow> = sqlx::query_as(
        r#"
SELECT sg.id, sg.name, sg.description
FROM security_groups sg
JOIN vm_security_groups vsg ON vsg.security_group_id = sg.id
WHERE vsg.vm_id = $1
ORDER BY sg.name
        "#,
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn rule_set_for_vm(
    pool: &PgPool,
    vm_id: Uuid,
) -> Result<Vec<SecurityGroupRule>, sqlx::Error> {
    let rows: Vec<SecurityGroupRuleRow> = sqlx::query_as(
        r#"
SELECT sgr.id,
       sgr.security_group_id,
       sgr.direction,
       sgr.protocol,
       sgr.cidr::text,
       sgr.port_start,
       sgr.port_end,
       sgr.description
FROM security_group_rules sgr
JOIN vm_security_groups vsg ON vsg.security_group_id = sgr.security_group_id
WHERE vsg.vm_id = $1
ORDER BY sgr.direction, sgr.protocol, sgr.cidr NULLS FIRST, sgr.port_start NULLS FIRST, sgr.id
        "#,
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn add_to_vm(
    tx: &mut PgTransaction<'_>,
    vm_id: Uuid,
    security_group_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
INSERT INTO vm_security_groups (vm_id, security_group_id)
VALUES ($1, $2)
ON CONFLICT DO NOTHING
        "#,
    )
    .bind(vm_id)
    .bind(security_group_id)
    .execute(tx.as_mut())
    .await?;

    Ok(())
}

pub async fn remove_from_vm(
    tx: &mut PgTransaction<'_>,
    vm_id: Uuid,
    security_group_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM vm_security_groups WHERE vm_id = $1 AND security_group_id = $2")
        .bind(vm_id)
        .bind(security_group_id)
        .execute(tx.as_mut())
        .await?;

    Ok(())
}

pub async fn list_vm_ids(pool: &PgPool, security_group_id: Uuid) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT vm_id FROM vm_security_groups WHERE security_group_id = $1 ORDER BY vm_id",
    )
    .bind(security_group_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|(vm_id,)| vm_id).collect())
}
