use super::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub port: i32,
    pub status: Status,
    pub host_user: String,

    #[serde(skip_deserializing)]
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewHost {
    pub name: String,
    pub address: String,
    pub port: i32,
    pub host_user: String,
    pub password: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Status {
    Unknown,
    Down,
    Installing,
    Initializing,
    Up,
}

#[derive(Error, Debug)]
pub enum HostError {
    #[error("Failed to list hosts: {0}")]
    List(#[from] sqlx::Error),

    #[error("Failed to add host: {0}, error: {1}")]
    Add(String, sqlx::Error),

    #[error("Can't find host: {0}, error: {1}")]
    Find(Uuid, sqlx::Error),

    #[error("Can't update host: {0}, error: {1}")]
    Updated(Uuid, sqlx::Error),

    #[error("{0}, error: {1}")]
    Other(String, sqlx::Error),
}

pub async fn list(pool: &PgPool) -> Result<Vec<Host>, HostError> {
    let hosts = sqlx::query_as!(
        Host,
        r#"
SELECT id, name, address, port, status as "status: _", host_user, password FROM hosts
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(HostError::List)?;

    Ok(hosts)
}

pub async fn add(pool: &PgPool, host: &NewHost) -> Result<Uuid, HostError> {
    let rec = sqlx::query!(
        r#"
INSERT INTO hosts (name, address, port, status, host_user, password)
VALUES ( $1, $2, $3, $4, $5, $6 )
RETURNING id
"#,
        host.name,
        host.address,
        host.port,
        Status::Down as Status,
        host.host_user,
        host.password
    )
    .fetch_one(pool)
    .await
    .map_err(|e| HostError::Add(host.name.to_owned(), e))?;

    Ok(rec.id)
}

pub async fn by_id(pool: &PgPool, host_id: &Uuid) -> Result<Host, HostError> {
    let host = sqlx::query_as!(
        Host,
        r#"
SELECT id, name, address, port, status as "status: _", host_user, password
FROM hosts
WHERE id = $1
        "#,
        host_id
    )
    .fetch_one(pool)
    .await
    .map_err(|e| HostError::Find(*host_id, e))?;

    Ok(host)
}

pub async fn by_status(pool: &PgPool, status: Status) -> Result<Vec<Host>, HostError> {
    tracing::info!("status {}", status.to_string());
    let hosts = sqlx::query_as!(
        Host,
        r#"
SELECT id, name, address, port, status as "status: _", host_user, password
FROM hosts
WHERE status = $1
        "#,
        status.to_string()
    )
    .fetch_all(pool)
    .await
    .map_err(|e| HostError::Other(String::from("Couldn't find host"), e))?;

    Ok(hosts)
}

pub async fn update_status(pool: &PgPool, host_id: Uuid, status: Status) -> anyhow::Result<bool> {
    let row_affected = sqlx::query!(
        r#"
UPDATE hosts
SET status = $1
WHERE id = $2
        "#,
        status as Status,
        host_id
    )
    .execute(pool)
    .await
    .map_err(|e| HostError::Updated(host_id, e))?
    .rows_affected();

    Ok(row_affected > 0)
}
