use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type, types::Uuid};
use strum_macros::{Display, EnumString};
use validator::{Validate, ValidationError, ValidationErrors};

use crate::errors;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub port: i32,
    pub status: HostStatus,
    pub host_user: String,

    #[serde(skip_deserializing)]
    pub password: Vec<u8>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "host_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HostStatus {
    Unknown,
    Down,
    Installing,
    InstallationFailed,
    Initializing,
    Up,
}

#[derive(Serialize, Deserialize, Debug, Clone, Validate)]
pub struct NewHost {
    #[validate(length(min = 1, max = 255))]
    pub name: String,
    pub address: String,

    #[validate(range(min = 1, max = 65535))]
    pub port: i32,

    pub host_user: String,
    pub password: String,
}

impl NewHost {
    pub async fn validate_unique_name(
        &self,
        pool: &PgPool,
        name: &str,
    ) -> Result<(), errors::Error> {
        let host = by_name(pool, name).await.map_err(errors::Error::Sqlx)?;

        if host.is_some() {
            let mut errors = ValidationErrors::new();
            errors.add("name", ValidationError::new("unique_name"));
            return Err(errors::Error::InvalidEntity(errors));
        }

        Ok(())
    }
}

pub async fn list(pool: &PgPool) -> Result<Vec<Host>, sqlx::Error> {
    let hosts = sqlx::query_as!(
        Host,
        r#"
        SELECT id, name, address, port, host_user, password, status as "status: _"
        FROM hosts
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(hosts)
}

// add adds a new host and returns its generated id
pub async fn add(pool: &PgPool, host: &NewHost) -> Result<Uuid, sqlx::Error> {
    let host_status = HostStatus::Down;
    let id = sqlx::query!(
        r#"
        INSERT INTO hosts (name, address, port, host_user, password, status)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
        host.name,
        host.address,
        host.port,
        host.host_user,
        host.password.as_bytes(),
        host_status as HostStatus,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Error adding host: {}", e);
        e
    })?
    .id;

    Ok(id)
}

// TODO: figure out how to not fetch the entire host. Maybe with SELECT exists()?
pub async fn by_name(pool: &PgPool, name: &str) -> Result<Option<Host>, sqlx::Error> {
    let host = sqlx::query_as!(
        Host,
        r#"
        SELECT id, name, address, port, host_user, password, status as "status: _"
        FROM hosts
        WHERE name = $1
        "#,
        name,
    )
    .fetch_optional(pool)
    .await?;

    Ok(host)
}
