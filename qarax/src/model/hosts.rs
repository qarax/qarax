use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, Type, types::Uuid};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use validator::{Validate, ValidationError, ValidationErrors};

use crate::errors;

/// Build a Host from a sqlx Row containing all host columns.
fn host_from_row(r: &sqlx::postgres::PgRow) -> Host {
    Host {
        id: r.get("id"),
        name: r.get("name"),
        address: r.get("address"),
        port: r.get("port"),
        host_user: r.get("host_user"),
        password: r.get("password"),
        status: r.get("status"),
        cloud_hypervisor_version: r.get("cloud_hypervisor_version"),
        kernel_version: r.get("kernel_version"),
        total_cpus: r.get("total_cpus"),
        total_memory_bytes: r.get("total_memory_bytes"),
        available_memory_bytes: r.get("available_memory_bytes"),
        load_average: r.get("load_average"),
        disk_total_bytes: r.get("disk_total_bytes"),
        disk_available_bytes: r.get("disk_available_bytes"),
        resources_updated_at: r.get("resources_updated_at"),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub port: i32,
    pub status: HostStatus,
    pub host_user: String,

    #[serde(skip_deserializing)]
    pub password: Vec<u8>,

    pub cloud_hypervisor_version: Option<String>,
    pub kernel_version: Option<String>,

    // Resource metrics
    pub total_cpus: Option<i32>,
    pub total_memory_bytes: Option<i64>,
    pub available_memory_bytes: Option<i64>,
    pub load_average: Option<f64>,
    pub disk_total_bytes: Option<i64>,
    pub disk_available_bytes: Option<i64>,
    pub resources_updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
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

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct UpdateHostRequest {
    pub status: HostStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeployHostRequest {
    /// Fully-qualified bootc image reference to deploy on the host.
    pub image: String,
    /// SSH port used to reach the host. Defaults to 22.
    pub ssh_port: Option<u16>,
    /// SSH user override. Defaults to the host's registered `host_user`.
    pub ssh_user: Option<String>,
    /// Optional SSH password override for this deployment request.
    pub ssh_password: Option<String>,
    /// Optional SSH private key path on the qarax control-plane host.
    pub ssh_private_key_path: Option<String>,
    /// Install bootc before switching image. Defaults to true.
    pub install_bootc: Option<bool>,
    /// Reboot after `bootc switch`. Defaults to true.
    pub reboot: Option<bool>,
}

impl DeployHostRequest {
    pub fn validate(&self) -> Result<(), crate::errors::Error> {
        if self.image.trim().is_empty() {
            return Err(crate::errors::Error::UnprocessableEntity(
                "image is required".to_string(),
            ));
        }

        if self.ssh_password.is_some() && self.ssh_private_key_path.is_some() {
            return Err(crate::errors::Error::UnprocessableEntity(
                "provide either ssh_password or ssh_private_key_path, but not both".to_string(),
            ));
        }

        if let Some(user) = &self.ssh_user
            && user.trim().is_empty()
        {
            return Err(crate::errors::Error::UnprocessableEntity(
                "ssh_user cannot be empty".to_string(),
            ));
        }

        if let Some(path) = &self.ssh_private_key_path
            && path.trim().is_empty()
        {
            return Err(crate::errors::Error::UnprocessableEntity(
                "ssh_private_key_path cannot be empty".to_string(),
            ));
        }

        if let Some(port) = self.ssh_port
            && port == 0
        {
            return Err(crate::errors::Error::UnprocessableEntity(
                "ssh_port must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }

    pub fn ssh_port(&self) -> u16 {
        self.ssh_port.unwrap_or(22)
    }

    pub fn install_bootc(&self) -> bool {
        self.install_bootc.unwrap_or(true)
    }

    pub fn reboot(&self) -> bool {
        self.reboot.unwrap_or(true)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Validate, ToSchema)]
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
    pub async fn validate_unique_name(&self, pool: &PgPool) -> Result<(), errors::Error> {
        let host = by_name(pool, &self.name)
            .await
            .map_err(errors::Error::Sqlx)?;

        if host.is_some() {
            let mut errors = ValidationErrors::new();
            errors.add("name", ValidationError::new("unique_name"));
            return Err(errors::Error::InvalidEntity(errors));
        }

        Ok(())
    }
}

pub async fn list(pool: &PgPool) -> Result<Vec<Host>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, address, port, host_user, password, status, cloud_hypervisor_version, kernel_version, total_cpus, total_memory_bytes, available_memory_bytes, load_average, disk_total_bytes, disk_available_bytes, resources_updated_at FROM hosts",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(host_from_row).collect())
}

// add adds a new host and returns its generated id
pub async fn add(pool: &PgPool, host: &NewHost) -> Result<Uuid, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO hosts (name, address, port, host_user, password, status)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
    )
    .bind(&host.name)
    .bind(&host.address)
    .bind(host.port)
    .bind(&host.host_user)
    .bind(host.password.as_bytes())
    .bind(HostStatus::Down)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Error adding host: {}", e);
        e
    })?;

    Ok(row.get("id"))
}

pub async fn update_status(pool: &PgPool, id: Uuid, status: HostStatus) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE hosts SET status = $1 WHERE id = $2")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Returns a host by id, or `Error::NotFound` if it does not exist.
pub async fn require_by_id(pool: &PgPool, id: Uuid) -> Result<Host, crate::errors::Error> {
    get_by_id(pool, id)
        .await?
        .ok_or(crate::errors::Error::NotFound)
}

/// Returns a host by id, if it exists.
pub async fn get_by_id(pool: &PgPool, host_id: Uuid) -> Result<Option<Host>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, address, port, host_user, password, status, cloud_hypervisor_version, kernel_version, total_cpus, total_memory_bytes, available_memory_bytes, load_average, disk_total_bytes, disk_available_bytes, resources_updated_at FROM hosts WHERE id = $1",
    )
    .bind(host_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| host_from_row(&r)))
}

/// Pick an UP host for VM scheduling with resource-aware placement.
///
/// Picks the UP host with the lowest load average that has enough memory.
/// vCPU oversubscription is allowed — the hypervisor scheduler handles
/// contention, and load_average-based ordering naturally prefers less busy hosts.
pub async fn pick_up_host(
    pool: &PgPool,
    requested_memory: i64,
) -> Result<Option<Host>, sqlx::Error> {
    let row = sqlx::query(
        r#"
SELECT id, name, address, port, host_user, password,
       status, cloud_hypervisor_version, kernel_version,
       total_cpus, total_memory_bytes, available_memory_bytes,
       load_average, disk_total_bytes, disk_available_bytes,
       resources_updated_at
FROM hosts
WHERE status = 'UP'
  AND (available_memory_bytes IS NULL OR available_memory_bytes >= $1)
ORDER BY load_average ASC NULLS LAST
LIMIT 1
        "#,
    )
    .bind(requested_memory)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| host_from_row(&r)))
}

/// Pick an UP host attached to a specific storage pool with resource-aware placement.
pub async fn pick_up_host_for_pool(
    pool: &PgPool,
    pool_id: Uuid,
    requested_memory: i64,
) -> Result<Option<Host>, sqlx::Error> {
    let row = sqlx::query(
        r#"
SELECT h.id, h.name, h.address, h.port, h.host_user, h.password,
       h.status, h.cloud_hypervisor_version, h.kernel_version,
       h.total_cpus, h.total_memory_bytes, h.available_memory_bytes,
       h.load_average, h.disk_total_bytes, h.disk_available_bytes,
       h.resources_updated_at
FROM hosts h
JOIN host_storage_pools hsp ON hsp.host_id = h.id
WHERE h.status = 'UP'
  AND hsp.storage_pool_id = $1
  AND (h.available_memory_bytes IS NULL OR h.available_memory_bytes >= $2)
ORDER BY h.load_average ASC NULLS LAST
LIMIT 1
        "#,
    )
    .bind(pool_id)
    .bind(requested_memory)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| host_from_row(&r)))
}

/// Pick an UP host that has enough available GPUs matching the given filters.
pub async fn pick_up_host_with_gpu(
    pool: &PgPool,
    requested_memory: i64,
    gpu_count: i32,
    gpu_vendor: Option<&str>,
    gpu_model: Option<&str>,
    min_vram_bytes: Option<i64>,
) -> Result<Option<Host>, sqlx::Error> {
    let row = sqlx::query(
        r#"
SELECT h.id, h.name, h.address, h.port, h.host_user, h.password,
       h.status, h.cloud_hypervisor_version, h.kernel_version,
       h.total_cpus, h.total_memory_bytes, h.available_memory_bytes,
       h.load_average, h.disk_total_bytes, h.disk_available_bytes,
       h.resources_updated_at
FROM hosts h
WHERE h.status = 'UP'
  AND (h.available_memory_bytes IS NULL OR h.available_memory_bytes >= $1)
  AND (
    SELECT COUNT(*) FROM host_gpus g
    WHERE g.host_id = h.id
      AND g.vm_id IS NULL
      AND ($3::VARCHAR IS NULL OR g.vendor = $3)
      AND ($4::VARCHAR IS NULL OR g.model = $4)
      AND ($5::BIGINT IS NULL OR g.vram_bytes >= $5)
  ) >= $2
ORDER BY h.load_average ASC NULLS LAST
LIMIT 1
        "#,
    )
    .bind(requested_memory)
    .bind(gpu_count as i64)
    .bind(gpu_vendor)
    .bind(gpu_model)
    .bind(min_vram_bytes)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| host_from_row(&r)))
}

/// Return all UP hosts.
pub async fn list_up(pool: &PgPool) -> Result<Vec<Host>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, address, port, host_user, password, status, cloud_hypervisor_version, kernel_version, total_cpus, total_memory_bytes, available_memory_bytes, load_average, disk_total_bytes, disk_available_bytes, resources_updated_at FROM hosts WHERE status = 'UP'",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| host_from_row(&r)).collect())
}

/// Update version information for a host (called after GetNodeInfo).
pub async fn update_versions(
    pool: &PgPool,
    id: Uuid,
    ch_version: &str,
    kernel_version: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE hosts SET cloud_hypervisor_version = $1, kernel_version = $2 WHERE id = $3",
    )
    .bind(ch_version)
    .bind(kernel_version)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update resource metrics for a host (called after GetNodeInfo).
#[allow(clippy::too_many_arguments)]
pub async fn update_resources(
    pool: &PgPool,
    id: Uuid,
    total_cpus: i32,
    total_memory_bytes: i64,
    available_memory_bytes: i64,
    load_average: f64,
    disk_total_bytes: i64,
    disk_available_bytes: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE hosts SET
    total_cpus = $1,
    total_memory_bytes = $2,
    available_memory_bytes = $3,
    load_average = $4,
    disk_total_bytes = $5,
    disk_available_bytes = $6,
    resources_updated_at = NOW()
WHERE id = $7
        "#,
    )
    .bind(total_cpus)
    .bind(total_memory_bytes)
    .bind(available_memory_bytes)
    .bind(load_average)
    .bind(disk_total_bytes)
    .bind(disk_available_bytes)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

// TODO: figure out how to not fetch the entire host. Maybe with SELECT exists()?
pub async fn by_name(pool: &PgPool, name: &str) -> Result<Option<Host>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, address, port, host_user, password, status, cloud_hypervisor_version, kernel_version, total_cpus, total_memory_bytes, available_memory_bytes, load_average, disk_total_bytes, disk_available_bytes, resources_updated_at FROM hosts WHERE name = $1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| host_from_row(&r)))
}

#[cfg(test)]
mod tests {
    use super::DeployHostRequest;

    #[test]
    fn deploy_request_rejects_empty_image() {
        let request = DeployHostRequest {
            image: "   ".to_string(),
            ssh_port: None,
            ssh_user: None,
            ssh_password: None,
            ssh_private_key_path: None,
            install_bootc: None,
            reboot: None,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn deploy_request_rejects_multiple_auth_methods() {
        let request = DeployHostRequest {
            image: "quay.io/example/qarax-vmm:v1".to_string(),
            ssh_port: Some(22),
            ssh_user: Some("root".to_string()),
            ssh_password: Some("secret".to_string()),
            ssh_private_key_path: Some("/tmp/id_rsa".to_string()),
            install_bootc: Some(true),
            reboot: Some(true),
        };

        assert!(request.validate().is_err());
    }
}
