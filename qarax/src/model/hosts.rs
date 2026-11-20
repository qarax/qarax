use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::{
    PgPool, Postgres, QueryBuilder, Transaction, Type,
    types::{Json, Uuid},
};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use validator::{Validate, ValidationError, ValidationErrors};

use crate::{errors, pagination::PageRequest};

/// The version of the control-plane binary, used to detect out-of-date nodes.
pub const CONTROL_PLANE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A `hosts` row as stored. `Host` adds the derived `update_available`.
#[derive(sqlx::FromRow)]
struct HostRow {
    id: Uuid,
    name: String,
    address: String,
    port: i32,
    host_user: String,
    credential_ref: Option<String>,
    status: HostStatus,
    cloud_hypervisor_version: Option<String>,
    firecracker_version: Option<String>,
    kernel_version: Option<String>,
    node_version: Option<String>,
    last_deployed_image: Option<String>,
    reservation_class: Option<String>,
    placement_labels: Json<BTreeMap<String, String>>,
    architecture: Option<String>,
    total_cpus: Option<i32>,
    total_memory_bytes: Option<i64>,
    available_memory_bytes: Option<i64>,
    load_average: Option<f64>,
    disk_total_bytes: Option<i64>,
    disk_available_bytes: Option<i64>,
    resources_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    installation_error: Option<String>,
}

impl From<HostRow> for Host {
    fn from(r: HostRow) -> Self {
        let update_available = r
            .node_version
            .as_deref()
            .is_some_and(|v| v != CONTROL_PLANE_VERSION);
        Host {
            id: r.id,
            name: r.name,
            address: r.address,
            port: r.port,
            host_user: r.host_user,
            credential_ref: r.credential_ref,
            status: r.status,
            cloud_hypervisor_version: r.cloud_hypervisor_version,
            firecracker_version: r.firecracker_version,
            kernel_version: r.kernel_version,
            node_version: r.node_version,
            last_deployed_image: r.last_deployed_image,
            update_available,
            reservation_class: r.reservation_class,
            placement_labels: r.placement_labels.0,
            architecture: r.architecture,
            total_cpus: r.total_cpus,
            total_memory_bytes: r.total_memory_bytes,
            available_memory_bytes: r.available_memory_bytes,
            load_average: r.load_average,
            disk_total_bytes: r.disk_total_bytes,
            disk_available_bytes: r.disk_available_bytes,
            resources_updated_at: r.resources_updated_at,
            installation_error: r.installation_error,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub port: i32,
    pub status: HostStatus,
    pub host_user: String,

    #[serde(skip)]
    #[schema(ignore)]
    pub credential_ref: Option<String>,

    pub cloud_hypervisor_version: Option<String>,
    pub firecracker_version: Option<String>,
    pub kernel_version: Option<String>,
    /// Version of the qarax-node agent running on this host.
    pub node_version: Option<String>,
    /// Last bootc image deployed to this host via the `/deploy` endpoint.
    pub last_deployed_image: Option<String>,
    /// True when `node_version` differs from the control-plane version.
    pub update_available: bool,
    /// Optional reservation class used by policy-based placement.
    pub reservation_class: Option<String>,
    /// Arbitrary placement labels used by policy-based placement.
    #[serde(default)]
    pub placement_labels: BTreeMap<String, String>,
    pub architecture: Option<String>,

    // Resource metrics
    pub total_cpus: Option<i32>,
    pub total_memory_bytes: Option<i64>,
    pub available_memory_bytes: Option<i64>,
    pub load_average: Option<f64>,
    pub disk_total_bytes: Option<i64>,
    pub disk_available_bytes: Option<i64>,
    pub resources_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Why the last deploy or upgrade failed. Set only while the status is
    /// `installation_failed`.
    pub installation_error: Option<String>,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Host")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("address", &self.address)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "host_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HostStatus {
    /// Not written by this version; kept for rows and clients that have it.
    Unknown,
    /// Registered but not probed yet, or a probe of an `Up` host failed.
    Down,
    /// A deploy or upgrade is running. Only that deploy moves the host on,
    /// to `Up` or `InstallationFailed`.
    Installing,
    /// The last deploy or upgrade failed; `installation_error` says why.
    InstallationFailed,
    /// Not written by this version; kept for rows and clients that have it.
    Initializing,
    Maintenance,
    Up,
}

impl HostStatus {
    /// Every status but `Installing`: what any writer other than the running
    /// deploy may move a host from.
    pub const NOT_INSTALLING: &[HostStatus] = &[
        HostStatus::Unknown,
        HostStatus::Down,
        HostStatus::InstallationFailed,
        HostStatus::Initializing,
        HostStatus::Maintenance,
        HostStatus::Up,
    ];

    /// Whether an operator may set this status with `PATCH /hosts/{id}`.
    /// The others record what qarax itself did to the host.
    pub fn operator_settable(&self) -> bool {
        match self {
            HostStatus::Up | HostStatus::Down | HostStatus::Maintenance => true,
            HostStatus::Unknown
            | HostStatus::Installing
            | HostStatus::InstallationFailed
            | HostStatus::Initializing => false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct UpdateHostRequest {
    pub status: HostStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, ToSchema)]
pub struct UpdateHostPlacementRequest {
    /// Optional reservation class this host belongs to.
    pub reservation_class: Option<String>,
    /// Arbitrary placement labels for scheduler filters and preferences.
    #[serde(default)]
    pub placement_labels: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, ToSchema)]
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

impl std::fmt::Debug for DeployHostRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeployHostRequest")
            .field("image", &self.image)
            .field("ssh_port", &self.ssh_port)
            .field("install_bootc", &self.install_bootc)
            .field("reboot", &self.reboot)
            .finish_non_exhaustive()
    }
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

#[derive(Serialize, Deserialize, Clone, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewHost {
    #[validate(length(min = 1, max = 255))]
    pub name: String,
    pub address: String,

    #[validate(range(min = 1, max = 65535))]
    pub port: i32,

    pub host_user: String,
    /// External SSH password reference (`env://NAME` or `file:///absolute/path`).
    /// Accepted on input but never returned by host APIs.
    pub credential_ref: Option<String>,
    /// Optional reservation class this host belongs to.
    pub reservation_class: Option<String>,
    /// Arbitrary placement labels for scheduler filters and preferences.
    #[serde(default)]
    pub placement_labels: BTreeMap<String, String>,
}

impl std::fmt::Debug for NewHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NewHost")
            .field("name", &self.name)
            .field("address", &self.address)
            .field("port", &self.port)
            .field("host_user", &self.host_user)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SchedulingRequest {
    pub memory_bytes: i64,
    pub vcpus: i32,
    pub disk_bytes: i64,
    pub architecture: Option<String>,
    pub storage_pool_id: Option<Uuid>,
    pub required_network_ids: Vec<Uuid>,
    /// The VM boots an OCI image: only hosts with an active OverlayBD pool
    /// (and so an overlaybd-tcmu daemon) can run it.
    #[serde(default)]
    pub requires_overlaybd: bool,
    pub gpu: Option<GpuRequest>,
    pub placement_policy: Option<crate::model::vms::PlacementPolicy>,
    #[serde(default)]
    pub excluded_host_ids: Vec<Uuid>,
    /// Only this host can take the VMs: a fork runs where its source does.
    #[serde(default)]
    pub required_host_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GpuRequest {
    pub count: i32,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub min_vram_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HostResourceCapacity {
    pub host_id: Uuid,
    pub architecture: Option<String>,
    pub total_cpus: Option<i32>,
    pub allocated_vcpus: i64,
    pub total_memory_bytes: Option<i64>,
    /// The memory of the host's VMs that aren't shut down: each one's memory
    /// size, a fork's the memory it holds of its own plus headroom (see
    /// `scheduling.fork_memory_headroom_ratio`).
    pub allocated_memory_bytes: i64,
    pub available_memory_bytes: Option<i64>,
    pub disk_total_bytes: Option<i64>,
    pub disk_available_bytes: Option<i64>,
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

    pub fn validate_placement(&self) -> Result<(), errors::Error> {
        validate_placement_fields(self.reservation_class.as_deref(), &self.placement_labels)
    }

    pub fn validate_credentials(&self) -> Result<(), errors::Error> {
        if let Some(credential_ref) = self.credential_ref.as_deref() {
            crate::secret_provider::validate_credential_ref(credential_ref)
                .map_err(|error| errors::Error::UnprocessableEntity(error.to_string()))?;
        }
        Ok(())
    }
}

impl UpdateHostPlacementRequest {
    pub fn validate(&self) -> Result<(), errors::Error> {
        validate_placement_fields(self.reservation_class.as_deref(), &self.placement_labels)
    }
}

fn validate_placement_fields(
    reservation_class: Option<&str>,
    placement_labels: &BTreeMap<String, String>,
) -> Result<(), errors::Error> {
    if let Some(class) = reservation_class
        && class.trim().is_empty()
    {
        return Err(errors::Error::UnprocessableEntity(
            "reservation_class cannot be empty".to_string(),
        ));
    }

    for (key, value) in placement_labels {
        if key.trim().is_empty() {
            return Err(errors::Error::UnprocessableEntity(
                "placement label keys cannot be empty".to_string(),
            ));
        }
        if value.trim().is_empty() {
            return Err(errors::Error::UnprocessableEntity(format!(
                "placement label '{}' cannot have an empty value",
                key
            )));
        }
    }

    Ok(())
}

/// Ordered by name.
pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
    architecture_filter: Option<&str>,
    page: &PageRequest,
) -> Result<Vec<Host>, sqlx::Error> {
    let rows = sqlx::query_as!(
        HostRow,
        r#"
SELECT id, name, address, port, host_user, credential_ref, status AS "status: _",
       cloud_hypervisor_version, firecracker_version, kernel_version, node_version,
       last_deployed_image, reservation_class, placement_labels AS "placement_labels: _",
       architecture, total_cpus, total_memory_bytes, available_memory_bytes, load_average,
       disk_total_bytes, disk_available_bytes, resources_updated_at, installation_error
FROM hosts
WHERE ($1::text IS NULL OR name = $1) AND ($2::text IS NULL OR architecture = $2)
  AND ($3::text IS NULL OR (name, id) > ($3, $4::uuid))
ORDER BY name, id
LIMIT $5
        "#,
        name_filter,
        architecture_filter,
        page.after_key(),
        page.after_id(),
        page.fetch_limit(),
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Host::from).collect())
}

// add adds a new host and returns its generated id
pub async fn add(pool: &PgPool, host: &NewHost) -> Result<Uuid, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        INSERT INTO hosts (
            name,
            address,
            port,
            host_user,
            credential_ref,
            status,
            reservation_class,
            placement_labels
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id
        "#,
        &host.name,
        &host.address,
        host.port,
        &host.host_user,
        host.credential_ref.as_deref(),
        HostStatus::Down as HostStatus,
        host.reservation_class.as_deref(),
        Json(&host.placement_labels) as _
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Error adding host: {}", e);
        e
    })?;

    Ok(row.id)
}

/// Move a host to `to` if its status is one of `from`, and return whether it
/// did. Every host status write goes through here (or [`fail_installation`]),
/// so a writer acting on an earlier read can't overwrite a newer status.
pub async fn transition(
    pool: &PgPool,
    id: Uuid,
    from: &[HostStatus],
    to: HostStatus,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE hosts SET status = $3, installation_error = NULL WHERE id = $1 AND status = ANY($2)",
        id,
        from as &[HostStatus],
        to as HostStatus
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

/// `Installing` -> `InstallationFailed`, recording why.
pub async fn fail_installation(pool: &PgPool, id: Uuid, error: &str) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE hosts SET status = 'INSTALLATION_FAILED', installation_error = $2 \
         WHERE id = $1 AND status = 'INSTALLING'",
        id,
        error
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

pub async fn update_placement(
    pool: &PgPool,
    id: Uuid,
    request: &UpdateHostPlacementRequest,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE hosts SET reservation_class = $1, placement_labels = $2 WHERE id = $3",
        request.reservation_class.as_deref(),
        Json(&request.placement_labels) as _,
        id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Names of the VMs placed on the host.
pub async fn vm_names_on(pool: &PgPool, host_id: Uuid) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT name FROM vms WHERE host_id = $1 ORDER BY name",
        host_id
    )
    .fetch_all(pool)
    .await
}

/// Delete a host. Its pool, network, GPU and NUMA rows cascade. Pending
/// storage cleanups and managed objects list hosts in arrays, which don't
/// cascade, so drop the host from those too: a cleanup aimed at a host that
/// no longer exists would otherwise be retried forever.
pub async fn delete(pool: &PgPool, host_id: Uuid) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        "UPDATE storage_cleanup SET host_ids = array_remove(host_ids, $1) WHERE $1 = ANY(host_ids)",
        host_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE storage_objects SET managed_host_ids = array_remove(managed_host_ids, $1) WHERE $1 = ANY(managed_host_ids)",
        host_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!("DELETE FROM hosts WHERE id = $1", host_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

/// Returns a host by id, or `Error::NotFound` if it does not exist.
pub async fn require_by_id(pool: &PgPool, id: Uuid) -> Result<Host, crate::errors::Error> {
    get_by_id(pool, id)
        .await?
        .ok_or(crate::errors::Error::NotFound)
}

/// Returns a host by id, if it exists.
pub async fn get_by_id(pool: &PgPool, host_id: Uuid) -> Result<Option<Host>, sqlx::Error> {
    let row = sqlx::query_as!(
        HostRow,
        r#"
SELECT id, name, address, port, host_user, credential_ref, status AS "status: _",
       cloud_hypervisor_version, firecracker_version, kernel_version, node_version,
       last_deployed_image, reservation_class, placement_labels AS "placement_labels: _",
       architecture, total_cpus, total_memory_bytes, available_memory_bytes, load_average,
       disk_total_bytes, disk_available_bytes, resources_updated_at, installation_error
FROM hosts
WHERE id = $1
        "#,
        host_id,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Host::from))
}

pub async fn pick_host_tx(
    tx: &mut Transaction<'_, Postgres>,
    request: &SchedulingRequest,
    config: &crate::configuration::SchedulingSettings,
) -> Result<Option<Host>, sqlx::Error> {
    if let Some(host_id) = request.required_host_id {
        // With one candidate, wait for a placement on it to commit rather than
        // skip it as the query below would (SKIP LOCKED). Lock it in a
        // statement of its own: a statement that waited for a row lock still
        // reads the rest from the snapshot it started with, which would miss
        // that placement's VMs.
        sqlx::query_scalar!("SELECT id FROM hosts WHERE id = $1 FOR UPDATE", host_id)
            .fetch_optional(tx.as_mut())
            .await?;
    }

    let mut qb = QueryBuilder::<Postgres>::new(
        r#"
SELECT h.id, h.name, h.address, h.port, h.host_user, h.credential_ref,
        h.status, h.cloud_hypervisor_version, h.firecracker_version, h.kernel_version,
       h.node_version, h.last_deployed_image, h.reservation_class, h.placement_labels,
       h.architecture,
       h.total_cpus, h.total_memory_bytes, h.available_memory_bytes,
       h.load_average, h.disk_total_bytes, h.disk_available_bytes,
       h.resources_updated_at,
       h.installation_error
FROM hosts h
LEFT JOIN (
    SELECT host_id,
           SUM(vm_memory_charge(memory_size, memory_copy_on_write, memory_private_bytes, "#
            .to_string(),
    );
    qb.push_bind(config.fork_memory_headroom_ratio);
    qb.push(
        r#"))::bigint AS allocated_memory_bytes,
           SUM(boot_vcpus)::bigint AS allocated_vcpus
    FROM vms
    WHERE status NOT IN ('SHUTDOWN', 'UNKNOWN')
      AND host_id IS NOT NULL
    GROUP BY host_id
) alloc ON alloc.host_id = h.id
"#,
    );

    if request.storage_pool_id.is_some() {
        qb.push("JOIN host_storage_pools hsp ON hsp.host_id = h.id ");
    }

    qb.push("WHERE h.status = 'UP' ");

    if let Some(storage_pool_id) = request.storage_pool_id {
        qb.push("AND hsp.storage_pool_id = ");
        qb.push_bind(storage_pool_id);
        qb.push(' ');
    }

    if !request.excluded_host_ids.is_empty() {
        qb.push("AND NOT (h.id = ANY(");
        qb.push_bind(&request.excluded_host_ids);
        qb.push(")) ");
    }

    if let Some(host_id) = request.required_host_id {
        qb.push("AND h.id = ");
        qb.push_bind(host_id);
        qb.push(' ');
    }

    for network_id in &request.required_network_ids {
        qb.push("AND EXISTS (SELECT 1 FROM host_networks hn WHERE hn.host_id = h.id AND hn.network_id = ");
        qb.push_bind(*network_id);
        qb.push(") ");
    }

    if request.requires_overlaybd {
        qb.push(
            "AND EXISTS (SELECT 1 FROM host_storage_pools obd_hsp \
             JOIN storage_pools obd_sp ON obd_sp.id = obd_hsp.storage_pool_id \
             WHERE obd_hsp.host_id = h.id AND obd_sp.pool_type = 'OVERLAYBD' \
             AND obd_sp.status = 'ACTIVE') ",
        );
    }

    if let Some(architecture) = request.architecture.as_deref() {
        qb.push("AND (h.architecture IS NULL OR h.architecture = ");
        qb.push_bind(architecture);
        qb.push(") ");
    }

    if let Some(policy) = request.placement_policy.as_ref() {
        if let Some(reservation_class) = policy.reservation_class.as_deref() {
            qb.push("AND h.reservation_class = ");
            qb.push_bind(reservation_class);
            qb.push(' ');
        }

        if !policy.required_host_labels.is_empty() {
            qb.push("AND h.placement_labels @> CAST(");
            qb.push_bind(serde_json::json!(policy.required_host_labels));
            qb.push(" AS jsonb) ");
        }

        if !policy.anti_affinity_tags.is_empty() {
            qb.push(
                "AND NOT EXISTS (
                    SELECT 1
                    FROM vms policy_vms
                    WHERE policy_vms.host_id = h.id
                      AND policy_vms.status NOT IN ('SHUTDOWN', 'UNKNOWN')
                      AND policy_vms.tags && ",
            );
            qb.push_bind(&policy.anti_affinity_tags);
            qb.push(") ");
        }
    }

    qb.push("AND (h.total_memory_bytes IS NULL OR ((h.total_memory_bytes::double precision * ");
    qb.push_bind(config.memory_oversubscription_ratio);
    qb.push(") - COALESCE(alloc.allocated_memory_bytes, 0)::double precision) >= ");
    qb.push_bind(request.memory_bytes as f64);
    qb.push(") ");

    qb.push("AND (h.total_cpus IS NULL OR ((h.total_cpus::double precision * ");
    qb.push_bind(config.cpu_oversubscription_ratio);
    qb.push(") - COALESCE(alloc.allocated_vcpus, 0)::double precision) >= ");
    qb.push_bind(request.vcpus as f64);
    qb.push(") ");

    qb.push("AND (h.available_memory_bytes IS NULL OR h.available_memory_bytes > ");
    qb.push_bind(config.memory_health_floor_bytes);
    qb.push(") ");

    qb.push("AND (h.disk_available_bytes IS NULL OR h.disk_available_bytes >= ");
    qb.push_bind(request.disk_bytes + config.disk_headroom_bytes);
    qb.push(") ");

    if let Some(gpu) = request.gpu.as_ref() {
        qb.push(
            "AND ((SELECT COUNT(*) FROM host_gpus g WHERE g.host_id = h.id AND g.vm_id IS NULL ",
        );
        if let Some(vendor) = gpu.vendor.as_deref() {
            qb.push("AND g.vendor = ");
            qb.push_bind(vendor);
            qb.push(' ');
        }
        if let Some(model) = gpu.model.as_deref() {
            qb.push("AND g.model = ");
            qb.push_bind(model);
            qb.push(' ');
        }
        if let Some(min_vram_bytes) = gpu.min_vram_bytes {
            qb.push("AND g.vram_bytes >= ");
            qb.push_bind(min_vram_bytes);
            qb.push(' ');
        }
        qb.push(") >= ");
        qb.push_bind(gpu.count as i64);
        qb.push(") ");
    }

    qb.push("ORDER BY ");
    if let Some(policy) = request.placement_policy.as_ref() {
        if !policy.preferred_host_labels.is_empty() {
            qb.push("(h.placement_labels @> CAST(");
            qb.push_bind(serde_json::json!(policy.preferred_host_labels));
            qb.push(" AS jsonb)) DESC, ");
        }

        if !policy.affinity_tags.is_empty() {
            qb.push(
                "(SELECT COUNT(*)
                  FROM vms policy_vms
                  WHERE policy_vms.host_id = h.id
                    AND policy_vms.status NOT IN ('SHUTDOWN', 'UNKNOWN')
                    AND policy_vms.tags @> ",
            );
            qb.push_bind(&policy.affinity_tags);
            qb.push(") DESC, ");
        }

        if !policy.spread_tags.is_empty() {
            qb.push(
                "(SELECT COUNT(*)
                  FROM vms policy_vms
                  WHERE policy_vms.host_id = h.id
                    AND policy_vms.status NOT IN ('SHUTDOWN', 'UNKNOWN')
                    AND policy_vms.tags @> ",
            );
            qb.push_bind(&policy.spread_tags);
            qb.push(") ASC, ");
        }
    }

    qb.push("h.load_average ASC NULLS LAST, h.name ASC LIMIT 1 FOR UPDATE OF h SKIP LOCKED");

    let row = qb
        .build_query_as::<HostRow>()
        .fetch_optional(tx.as_mut())
        .await?;
    Ok(row.map(Host::from))
}

/// Return all UP hosts.
pub async fn list_up(pool: &PgPool) -> Result<Vec<Host>, sqlx::Error> {
    let rows = sqlx::query_as!(
        HostRow,
        r#"
SELECT id, name, address, port, host_user, credential_ref, status AS "status: _",
       cloud_hypervisor_version, firecracker_version, kernel_version, node_version,
       last_deployed_image, reservation_class, placement_labels AS "placement_labels: _",
       architecture, total_cpus, total_memory_bytes, available_memory_bytes, load_average,
       disk_total_bytes, disk_available_bytes, resources_updated_at, installation_error
FROM hosts
WHERE status = 'UP'
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Host::from).collect())
}

/// Return all DOWN hosts (candidates for HA failover).
pub async fn list_down(pool: &PgPool) -> Result<Vec<Host>, sqlx::Error> {
    let rows = sqlx::query_as!(
        HostRow,
        r#"
SELECT id, name, address, port, host_user, credential_ref, status AS "status: _",
       cloud_hypervisor_version, firecracker_version, kernel_version, node_version,
       last_deployed_image, reservation_class, placement_labels AS "placement_labels: _",
       architecture, total_cpus, total_memory_bytes, available_memory_bytes, load_average,
       disk_total_bytes, disk_available_bytes, resources_updated_at, installation_error
FROM hosts
WHERE status = 'DOWN'
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Host::from).collect())
}

/// Return hosts that should continue receiving health/resource probes.
pub async fn list_probeable(pool: &PgPool) -> Result<Vec<Host>, sqlx::Error> {
    let rows = sqlx::query_as!(
        HostRow,
        r#"
SELECT id, name, address, port, host_user, credential_ref, status AS "status: _",
       cloud_hypervisor_version, firecracker_version, kernel_version, node_version,
       last_deployed_image, reservation_class, placement_labels AS "placement_labels: _",
       architecture, total_cpus, total_memory_bytes, available_memory_bytes, load_average,
       disk_total_bytes, disk_available_bytes, resources_updated_at, installation_error
FROM hosts
WHERE status IN ('UP', 'MAINTENANCE')
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Host::from).collect())
}

/// Update version information for a host (called after GetNodeInfo).
pub async fn update_versions(
    pool: &PgPool,
    id: Uuid,
    ch_version: &str,
    fc_version: Option<&str>,
    kernel_version: &str,
    node_version: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE hosts SET cloud_hypervisor_version = $1, firecracker_version = $2, kernel_version = $3, node_version = $4 WHERE id = $5",
        ch_version,
        fc_version,
        kernel_version,
        node_version,
        id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Persist the last bootc image deployed to this host (called after a successful deploy).
pub async fn set_last_deployed_image(
    pool: &PgPool,
    id: Uuid,
    image: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE hosts SET last_deployed_image = $1 WHERE id = $2",
        image,
        id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Update resource metrics for a host (called after GetNodeInfo).
#[allow(clippy::too_many_arguments)]
pub async fn update_resources(
    pool: &PgPool,
    id: Uuid,
    architecture: Option<&str>,
    total_cpus: i32,
    total_memory_bytes: i64,
    available_memory_bytes: i64,
    load_average: f64,
    disk_total_bytes: i64,
    disk_available_bytes: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
UPDATE hosts SET
    architecture = $1,
    total_cpus = $2,
    total_memory_bytes = $3,
    available_memory_bytes = $4,
    load_average = $5,
    disk_total_bytes = $6,
    disk_available_bytes = $7,
    resources_updated_at = NOW()
WHERE id = $8
        "#,
        architecture,
        total_cpus,
        total_memory_bytes,
        available_memory_bytes,
        load_average,
        disk_total_bytes,
        disk_available_bytes,
        id
    )
    .execute(pool)
    .await?;
    Ok(())
}

// TODO: figure out how to not fetch the entire host. Maybe with SELECT exists()?
pub async fn by_name(pool: &PgPool, name: &str) -> Result<Option<Host>, sqlx::Error> {
    let row = sqlx::query_as!(
        HostRow,
        r#"
SELECT id, name, address, port, host_user, credential_ref, status AS "status: _",
       cloud_hypervisor_version, firecracker_version, kernel_version, node_version,
       last_deployed_image, reservation_class, placement_labels AS "placement_labels: _",
       architecture, total_cpus, total_memory_bytes, available_memory_bytes, load_average,
       disk_total_bytes, disk_available_bytes, resources_updated_at, installation_error
FROM hosts
WHERE name = $1
        "#,
        name,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Host::from))
}

pub async fn get_resource_capacity(
    pool: &PgPool,
    host_id: Uuid,
    config: &crate::configuration::SchedulingSettings,
) -> Result<Option<HostResourceCapacity>, sqlx::Error> {
    sqlx::query_as!(
        HostResourceCapacity,
        r#"
SELECT h.id AS host_id,
       h.architecture,
       h.total_cpus,
       COALESCE(alloc.allocated_vcpus, 0)::bigint AS "allocated_vcpus!",
       h.total_memory_bytes,
       COALESCE(alloc.allocated_memory_bytes, 0)::bigint AS "allocated_memory_bytes!",
       h.available_memory_bytes,
       h.disk_total_bytes,
       h.disk_available_bytes
FROM hosts h
LEFT JOIN (
    SELECT host_id,
           SUM(vm_memory_charge(memory_size, memory_copy_on_write, memory_private_bytes, $2))::bigint
               AS allocated_memory_bytes,
           SUM(boot_vcpus)::bigint AS allocated_vcpus
    FROM vms
    WHERE status NOT IN ('SHUTDOWN', 'UNKNOWN')
      AND host_id IS NOT NULL
    GROUP BY host_id
) alloc ON alloc.host_id = h.id
WHERE h.id = $1
        "#,
        host_id,
        config.fork_memory_headroom_ratio
    )
    .fetch_optional(pool)
    .await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use uuid::Uuid;

    use super::{
        DeployHostRequest, GpuRequest, HostStatus, SchedulingRequest, UpdateHostPlacementRequest,
    };
    use crate::{
        configuration::SchedulingSettings,
        model::{
            host_gpus::{self, GpuDiscovery},
            networks::{self, NewNetwork},
            vms::{self, PlacementPolicy, ResolvedNewVm, VmStatus},
        },
        test_support::TestDatabase,
    };

    impl TestDatabase {
        async fn insert_up_host(
            &self,
            name: &str,
            architecture: Option<&str>,
            total_cpus: i32,
            total_memory_bytes: i64,
            disk_available_bytes: i64,
            load_average: f64,
        ) -> Uuid {
            let host_id = self.add_host(name, HostStatus::Up).await;
            super::update_resources(
                &self.pool,
                host_id,
                architecture,
                total_cpus,
                total_memory_bytes,
                total_memory_bytes,
                load_average,
                disk_available_bytes,
                disk_available_bytes,
            )
            .await
            .expect("Failed to persist host resources");

            host_id
        }

        async fn create_network(&self, name: &str, subnet: &str, gateway: &str) -> Uuid {
            networks::create(
                &self.pool,
                NewNetwork {
                    name: name.to_string(),
                    subnet: subnet.to_string(),
                    gateway: Some(gateway.to_string()),
                    dns: None,
                    vpc_name: None,
                    network_type: Some("isolated".to_string()),
                },
            )
            .await
            .expect("create network")
        }

        async fn set_host_placement(
            &self,
            host_id: Uuid,
            reservation_class: Option<&str>,
            placement_labels: BTreeMap<String, String>,
        ) {
            super::update_placement(
                &self.pool,
                host_id,
                &UpdateHostPlacementRequest {
                    reservation_class: reservation_class.map(str::to_string),
                    placement_labels,
                },
            )
            .await
            .expect("update host placement");
        }
    }

    fn scheduling_config() -> SchedulingSettings {
        SchedulingSettings {
            memory_oversubscription_ratio: 1.0,
            cpu_oversubscription_ratio: 1.0,
            disk_headroom_bytes: 10,
            memory_health_floor_bytes: 0,
            fork_memory_headroom_ratio: 0.25,
        }
    }

    fn resolved_vm(name: &str, memory_size: i64, boot_vcpus: i32) -> ResolvedNewVm {
        ResolvedNewVm {
            boot_vcpus,
            max_vcpus: boot_vcpus,
            memory_size,
            ..crate::test_support::resolved_vm(name)
        }
    }

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

    #[test]
    fn placement_validation_rejects_empty_reservation_class() {
        let request = UpdateHostPlacementRequest {
            reservation_class: Some("   ".to_string()),
            placement_labels: BTreeMap::new(),
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn placement_validation_rejects_empty_label_values() {
        let mut placement_labels = BTreeMap::new();
        placement_labels.insert("zone".to_string(), " ".to_string());
        let request = UpdateHostPlacementRequest {
            reservation_class: Some("general".to_string()),
            placement_labels,
        };

        assert!(request.validate().is_err());
    }

    #[tokio::test]
    async fn pick_host_respects_architecture_filter() {
        let db = TestDatabase::new().await;
        let x86_id = db
            .insert_up_host("x86-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;
        let arm_id = db
            .insert_up_host("arm-host", Some("aarch64"), 8, 16 * 1024, 10_000, 0.2)
            .await;

        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 1024,
                vcpus: 2,
                disk_bytes: 100,
                architecture: Some("aarch64".to_string()),
                storage_pool_id: None,
                required_network_ids: vec![],
                requires_overlaybd: false,
                gpu: None,
                placement_policy: None,
                excluded_host_ids: vec![],
                required_host_id: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, arm_id);
        assert_ne!(host.id, x86_id);
    }

    #[tokio::test]
    async fn pick_host_respects_disk_capacity() {
        let db = TestDatabase::new().await;
        let low_disk_id = db
            .insert_up_host("low-disk", Some("x86_64"), 8, 16 * 1024, 100, 0.1)
            .await;
        let high_disk_id = db
            .insert_up_host("high-disk", Some("x86_64"), 8, 16 * 1024, 2_000, 0.2)
            .await;

        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 1024,
                vcpus: 2,
                disk_bytes: 250,
                architecture: Some("x86_64".to_string()),
                storage_pool_id: None,
                required_network_ids: vec![],
                requires_overlaybd: false,
                gpu: None,
                placement_policy: None,
                excluded_host_ids: vec![],
                required_host_id: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, high_disk_id);
        assert_ne!(host.id, low_disk_id);
    }

    #[tokio::test]
    async fn pick_host_serializes_and_accounts_for_allocations() {
        let db = TestDatabase::new().await;
        let host_id = db
            .insert_up_host("sched-host", Some("x86_64"), 8, 1024, 10_000, 0.1)
            .await;
        let request = SchedulingRequest {
            memory_bytes: 800,
            vcpus: 2,
            disk_bytes: 0,
            architecture: Some("x86_64".to_string()),
            storage_pool_id: None,
            required_network_ids: vec![],
            requires_overlaybd: false,
            gpu: None,
            placement_policy: None,
            excluded_host_ids: vec![],
            required_host_id: None,
        };

        let mut tx1 = db.pool.begin().await.expect("begin tx1");
        let selected = super::pick_host_tx(&mut tx1, &request, &scheduling_config())
            .await
            .expect("pick host")
            .expect("expected host");
        assert_eq!(selected.id, host_id);
        vms::create_tx(&mut tx1, &resolved_vm("vm-a", 800, 2), Some(host_id))
            .await
            .expect("insert vm");

        let mut tx2 = db.pool.begin().await.expect("begin tx2");
        let skipped = super::pick_host_tx(&mut tx2, &request, &scheduling_config())
            .await
            .expect("pick host");
        assert!(skipped.is_none(), "locked host should be skipped");

        tx1.commit().await.expect("commit tx1");

        let mut tx3 = db.pool.begin().await.expect("begin tx3");
        let over_capacity = super::pick_host_tx(
            &mut tx3,
            &SchedulingRequest {
                memory_bytes: 300,
                ..request.clone()
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host");
        assert!(
            over_capacity.is_none(),
            "ledger should prevent over-allocation"
        );

        let mut tx4 = db.pool.begin().await.expect("begin tx4");
        let remaining_capacity = super::pick_host_tx(
            &mut tx4,
            &SchedulingRequest {
                memory_bytes: 200,
                ..request
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");
        assert_eq!(remaining_capacity.id, host_id);
    }

    #[tokio::test]
    async fn pick_host_with_a_required_host_considers_only_it() {
        let db = TestDatabase::new().await;
        // Busier and smaller: the scheduler alone would pick the other host.
        let required_id = db
            .insert_up_host("required-host", Some("x86_64"), 8, 1024, 10_000, 0.9)
            .await;
        db.insert_up_host("idle-host", Some("x86_64"), 8, 4096, 10_000, 0.1)
            .await;
        let request = SchedulingRequest {
            memory_bytes: 512,
            vcpus: 1,
            disk_bytes: 0,
            architecture: None,
            storage_pool_id: None,
            required_network_ids: vec![],
            requires_overlaybd: false,
            gpu: None,
            placement_policy: None,
            excluded_host_ids: vec![],
            required_host_id: Some(required_id),
        };

        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(&mut tx, &request, &scheduling_config())
            .await
            .expect("pick host")
            .expect("expected the required host");
        assert_eq!(host.id, required_id);
        drop(tx);

        // What doesn't fit on the required host goes nowhere else.
        let mut tx = db.pool.begin().await.expect("begin tx");
        let too_big = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 2048,
                ..request
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host");
        assert!(too_big.is_none(), "placed on {:?}", too_big.map(|h| h.name));
    }

    #[tokio::test]
    async fn pick_host_with_a_required_host_waits_for_a_placement_on_it() {
        let db = TestDatabase::new().await;
        let host_id = db
            .insert_up_host("pinned-host", Some("x86_64"), 8, 1024, 10_000, 0.1)
            .await;
        let request = SchedulingRequest {
            memory_bytes: 600,
            vcpus: 1,
            disk_bytes: 0,
            architecture: None,
            storage_pool_id: None,
            required_network_ids: vec![],
            requires_overlaybd: false,
            gpu: None,
            placement_policy: None,
            excluded_host_ids: vec![],
            required_host_id: None,
        };

        // A placement on the host, not committed yet.
        let mut tx1 = db.pool.begin().await.expect("begin tx1");
        super::pick_host_tx(&mut tx1, &request, &scheduling_config())
            .await
            .expect("pick host")
            .expect("expected host");
        vms::create_tx(&mut tx1, &resolved_vm("vm-a", 600, 1), Some(host_id))
            .await
            .expect("insert vm");

        let pool = db.pool.clone();
        let pinned = SchedulingRequest {
            required_host_id: Some(host_id),
            ..request
        };
        let waiting = tokio::spawn(async move {
            let mut tx2 = pool.begin().await.expect("begin tx2");
            super::pick_host_tx(&mut tx2, &pinned, &scheduling_config())
                .await
                .expect("pick host")
                .map(|host| host.id)
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !waiting.is_finished(),
            "the pinned placement didn't wait for the one in progress"
        );

        tx1.commit().await.expect("commit tx1");
        // Then it counts that placement: 600 + 600 doesn't fit in 1024.
        assert_eq!(waiting.await.expect("join"), None);
    }

    #[tokio::test]
    async fn a_fork_is_charged_the_memory_it_holds_of_its_own_plus_headroom() {
        async fn allocated(db: &TestDatabase, host_id: Uuid) -> i64 {
            super::get_resource_capacity(&db.pool, host_id, &scheduling_config())
                .await
                .expect("get capacity")
                .expect("expected capacity")
                .allocated_memory_bytes
        }

        let db = TestDatabase::new().await;
        let host_id = db
            .insert_up_host("fork-host", Some("x86_64"), 8, 4096, 10_000, 0.1)
            .await;
        let mut tx = db.pool.begin().await.expect("begin tx");
        let source = vms::create_tx(&mut tx, &resolved_vm("source", 1024, 1), Some(host_id))
            .await
            .expect("insert source");
        let fork = vms::create_fork_tx(&mut tx, source, "fork", true)
            .await
            .expect("insert fork");
        // A fork with a copy of its source's memory is charged in full.
        vms::create_fork_tx(&mut tx, source, "copy", false)
            .await
            .expect("insert copy");
        tx.commit().await.expect("commit tx");

        // Before its node has measured it, the fork is charged its headroom.
        assert_eq!(allocated(&db, host_id).await, 1024 + 256 + 1024);
        vms::record_private_memory(&db.pool, host_id, &[(fork, 100)])
            .await
            .expect("record memory");
        assert_eq!(allocated(&db, host_id).await, 1024 + 356 + 1024);

        // The scheduler places by the same charge.
        let room = 4096 - (1024 + 356 + 1024);
        for (memory_bytes, fits) in [(room, true), (room + 1, false)] {
            let mut tx = db.pool.begin().await.expect("begin tx");
            let picked = super::pick_host_tx(
                &mut tx,
                &SchedulingRequest {
                    memory_bytes,
                    vcpus: 1,
                    disk_bytes: 0,
                    architecture: None,
                    storage_pool_id: None,
                    required_network_ids: vec![],
                    requires_overlaybd: false,
                    gpu: None,
                    placement_policy: None,
                    excluded_host_ids: vec![],
                    required_host_id: Some(host_id),
                },
                &scheduling_config(),
            )
            .await
            .expect("pick host");
            assert_eq!(picked.is_some(), fits, "{memory_bytes} bytes");
        }

        // Never more than its memory size.
        vms::record_private_memory(&db.pool, host_id, &[(fork, 2000)])
            .await
            .expect("record memory");
        assert_eq!(allocated(&db, host_id).await, 3 * 1024);

        // Its memory stays its source's while it runs, until it moves on to a
        // hypervisor process of its own.
        vms::record_private_memory(&db.pool, host_id, &[(fork, 100)])
            .await
            .expect("record memory");
        vms::transition(&db.pool, fork, &[VmStatus::Pending], VmStatus::Running)
            .await
            .expect("start fork");
        assert_eq!(allocated(&db, host_id).await, 1024 + 356 + 1024);
        vms::transition(&db.pool, fork, &[VmStatus::Running], VmStatus::Migrating)
            .await
            .expect("migrate fork");
        assert_eq!(allocated(&db, host_id).await, 3 * 1024);
    }

    #[tokio::test]
    async fn get_resource_capacity_returns_integer_allocations() {
        let db = TestDatabase::new().await;
        let host_id = db
            .insert_up_host("capacity-host", Some("x86_64"), 8, 4096, 10_000, 0.1)
            .await;

        let mut tx = db.pool.begin().await.expect("begin tx");
        vms::create_tx(&mut tx, &resolved_vm("vm-a", 1536, 3), Some(host_id))
            .await
            .expect("insert vm");
        tx.commit().await.expect("commit tx");

        let capacity = super::get_resource_capacity(&db.pool, host_id, &scheduling_config())
            .await
            .expect("get capacity")
            .expect("expected capacity");

        assert_eq!(capacity.host_id, host_id);
        assert_eq!(capacity.allocated_vcpus, 3);
        assert_eq!(capacity.allocated_memory_bytes, 1536);
    }

    #[tokio::test]
    async fn pick_host_respects_required_network_attachments() {
        let db = TestDatabase::new().await;
        let attached_host_id = db
            .insert_up_host("attached-host", Some("x86_64"), 8, 16 * 1024, 10_000, 1.0)
            .await;
        let unattached_host_id = db
            .insert_up_host("unattached-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;
        let network_id = db
            .create_network("sched-net", "10.77.0.0/24", "10.77.0.1")
            .await;
        networks::attach_host(&db.pool, network_id, attached_host_id, "qtnet0", None)
            .await
            .expect("attach network to host");

        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 1024,
                vcpus: 2,
                disk_bytes: 100,
                architecture: Some("x86_64".to_string()),
                storage_pool_id: None,
                required_network_ids: vec![network_id],
                requires_overlaybd: false,
                gpu: None,
                placement_policy: None,
                excluded_host_ids: vec![],
                required_host_id: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, attached_host_id);
        assert_ne!(host.id, unattached_host_id);
    }

    #[tokio::test]
    async fn pick_host_requires_an_overlaybd_pool_for_image_vms() {
        let db = TestDatabase::new().await;
        let with_pool = db
            .insert_up_host("obd-host", Some("x86_64"), 8, 16 * 1024, 10_000, 1.0)
            .await;
        // Less loaded, so the scheduler would prefer it without the constraint.
        let without_pool = db
            .insert_up_host("plain-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;
        let pool_id = crate::model::storage_pools::create(
            &db.pool,
            crate::model::storage_pools::NewStoragePool {
                name: "obd-pool".into(),
                pool_type: crate::model::storage_pools::StoragePoolType::OverlayBd,
                config: serde_json::json!({"url": "http://registry:5000"}),
                capacity_bytes: None,
            },
        )
        .await
        .expect("create pool");
        crate::model::storage_pools::attach_host(&db.pool, pool_id, with_pool)
            .await
            .expect("attach pool");

        let request = |requires_overlaybd| SchedulingRequest {
            memory_bytes: 1024,
            vcpus: 1,
            disk_bytes: 100,
            architecture: Some("x86_64".to_string()),
            storage_pool_id: None,
            required_network_ids: vec![],
            requires_overlaybd,
            gpu: None,
            placement_policy: None,
            excluded_host_ids: vec![],
            required_host_id: None,
        };
        for (requires_overlaybd, expected) in [(true, with_pool), (false, without_pool)] {
            let mut tx = db.pool.begin().await.expect("begin tx");
            let host =
                super::pick_host_tx(&mut tx, &request(requires_overlaybd), &scheduling_config())
                    .await
                    .expect("pick host")
                    .expect("expected host");
            assert_eq!(host.id, expected, "requires_overlaybd={requires_overlaybd}");
        }
    }

    #[tokio::test]
    async fn pick_host_skips_excluded_hosts() {
        let db = TestDatabase::new().await;
        let excluded_host_id = db
            .insert_up_host("excluded-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;
        let allowed_host_id = db
            .insert_up_host("allowed-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.2)
            .await;

        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 1024,
                vcpus: 2,
                disk_bytes: 100,
                architecture: Some("x86_64".to_string()),
                storage_pool_id: None,
                required_network_ids: vec![],
                requires_overlaybd: false,
                gpu: None,
                placement_policy: None,
                excluded_host_ids: vec![excluded_host_id],
                required_host_id: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, allowed_host_id);
        assert_ne!(host.id, excluded_host_id);
    }

    #[tokio::test]
    async fn pick_host_respects_reservation_class() {
        let db = TestDatabase::new().await;
        let reserved_host_id = db
            .insert_up_host("reserved-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.9)
            .await;
        let general_host_id = db
            .insert_up_host("general-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;
        db.set_host_placement(reserved_host_id, Some("batch"), BTreeMap::new())
            .await;
        db.set_host_placement(general_host_id, Some("general"), BTreeMap::new())
            .await;

        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 1024,
                vcpus: 1,
                disk_bytes: 100,
                architecture: Some("x86_64".to_string()),
                storage_pool_id: None,
                required_network_ids: vec![],
                requires_overlaybd: false,
                gpu: None,
                placement_policy: Some(PlacementPolicy {
                    reservation_class: Some("batch".to_string()),
                    required_host_labels: BTreeMap::new(),
                    preferred_host_labels: BTreeMap::new(),
                    affinity_tags: vec![],
                    anti_affinity_tags: vec![],
                    spread_tags: vec![],
                }),
                excluded_host_ids: vec![],
                required_host_id: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, reserved_host_id);
        assert_ne!(host.id, general_host_id);
    }

    #[tokio::test]
    async fn pick_host_respects_required_host_labels() {
        let db = TestDatabase::new().await;
        let west_host_id = db
            .insert_up_host("west-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.9)
            .await;
        let east_host_id = db
            .insert_up_host("east-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;
        let mut west_labels = BTreeMap::new();
        west_labels.insert("zone".to_string(), "west".to_string());
        let mut east_labels = BTreeMap::new();
        east_labels.insert("zone".to_string(), "east".to_string());
        db.set_host_placement(west_host_id, None, west_labels).await;
        db.set_host_placement(east_host_id, None, east_labels).await;

        let mut required_labels = BTreeMap::new();
        required_labels.insert("zone".to_string(), "west".to_string());
        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 1024,
                vcpus: 1,
                disk_bytes: 100,
                architecture: Some("x86_64".to_string()),
                storage_pool_id: None,
                required_network_ids: vec![],
                requires_overlaybd: false,
                gpu: None,
                placement_policy: Some(PlacementPolicy {
                    reservation_class: None,
                    required_host_labels: required_labels,
                    preferred_host_labels: BTreeMap::new(),
                    affinity_tags: vec![],
                    anti_affinity_tags: vec![],
                    spread_tags: vec![],
                }),
                excluded_host_ids: vec![],
                required_host_id: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, west_host_id);
        assert_ne!(host.id, east_host_id);
    }

    #[tokio::test]
    async fn pick_host_respects_anti_affinity_tags() {
        let db = TestDatabase::new().await;
        let crowded_host_id = db
            .insert_up_host("crowded-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;
        let empty_host_id = db
            .insert_up_host("empty-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.9)
            .await;

        let mut tx = db.pool.begin().await.expect("begin tx");
        let mut existing_vm = resolved_vm("resident-vm", 256, 1);
        existing_vm.tags = vec!["tenant:blue".to_string()];
        vms::create_tx(&mut tx, &existing_vm, Some(crowded_host_id))
            .await
            .expect("insert resident vm");
        tx.commit().await.expect("commit tx");

        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 512,
                vcpus: 1,
                disk_bytes: 100,
                architecture: Some("x86_64".to_string()),
                storage_pool_id: None,
                required_network_ids: vec![],
                requires_overlaybd: false,
                gpu: None,
                placement_policy: Some(PlacementPolicy {
                    reservation_class: None,
                    required_host_labels: BTreeMap::new(),
                    preferred_host_labels: BTreeMap::new(),
                    affinity_tags: vec![],
                    anti_affinity_tags: vec!["tenant:blue".to_string()],
                    spread_tags: vec![],
                }),
                excluded_host_ids: vec![],
                required_host_id: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, empty_host_id);
        assert_ne!(host.id, crowded_host_id);
    }

    #[tokio::test]
    async fn pick_host_prefers_affinity_tags_over_lower_load() {
        let db = TestDatabase::new().await;
        let affinity_host_id = db
            .insert_up_host("affinity-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.9)
            .await;
        let lower_load_host_id = db
            .insert_up_host("lower-load-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;

        let mut tx = db.pool.begin().await.expect("begin tx");
        let mut existing_vm = resolved_vm("affinity-peer", 256, 1);
        existing_vm.tags = vec!["stack:web".to_string()];
        vms::create_tx(&mut tx, &existing_vm, Some(affinity_host_id))
            .await
            .expect("insert affinity vm");
        tx.commit().await.expect("commit tx");

        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 512,
                vcpus: 1,
                disk_bytes: 100,
                architecture: Some("x86_64".to_string()),
                storage_pool_id: None,
                required_network_ids: vec![],
                requires_overlaybd: false,
                gpu: None,
                placement_policy: Some(PlacementPolicy {
                    reservation_class: None,
                    required_host_labels: BTreeMap::new(),
                    preferred_host_labels: BTreeMap::new(),
                    affinity_tags: vec!["stack:web".to_string()],
                    anti_affinity_tags: vec![],
                    spread_tags: vec![],
                }),
                excluded_host_ids: vec![],
                required_host_id: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, affinity_host_id);
        assert_ne!(host.id, lower_load_host_id);
    }

    #[tokio::test]
    async fn pick_host_prefers_spread_tags_over_lower_load() {
        let db = TestDatabase::new().await;
        let crowded_host_id = db
            .insert_up_host("crowded-spread", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;
        let empty_host_id = db
            .insert_up_host("empty-spread", Some("x86_64"), 8, 16 * 1024, 10_000, 0.9)
            .await;

        let mut tx = db.pool.begin().await.expect("begin tx");
        for idx in 0..2 {
            let mut existing_vm = resolved_vm(&format!("spread-peer-{idx}"), 256, 1);
            existing_vm.tags = vec!["spread:web".to_string()];
            vms::create_tx(&mut tx, &existing_vm, Some(crowded_host_id))
                .await
                .expect("insert spread vm");
        }
        tx.commit().await.expect("commit tx");

        let mut tx = db.pool.begin().await.expect("begin tx");
        let host = super::pick_host_tx(
            &mut tx,
            &SchedulingRequest {
                memory_bytes: 512,
                vcpus: 1,
                disk_bytes: 100,
                architecture: Some("x86_64".to_string()),
                storage_pool_id: None,
                required_network_ids: vec![],
                requires_overlaybd: false,
                gpu: None,
                placement_policy: Some(PlacementPolicy {
                    reservation_class: None,
                    required_host_labels: BTreeMap::new(),
                    preferred_host_labels: BTreeMap::new(),
                    affinity_tags: vec![],
                    anti_affinity_tags: vec![],
                    spread_tags: vec!["spread:web".to_string()],
                }),
                excluded_host_ids: vec![],
                required_host_id: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, empty_host_id);
        assert_ne!(host.id, crowded_host_id);
    }

    #[tokio::test]
    async fn pick_host_requires_enough_free_matching_gpus() {
        let db = TestDatabase::new().await;
        let gpu_host = db
            .insert_up_host("gpu-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.9)
            .await;
        db.insert_up_host("cpu-host", Some("x86_64"), 8, 16 * 1024, 10_000, 0.1)
            .await;
        host_gpus::sync_gpus(
            &db.pool,
            gpu_host,
            &[GpuDiscovery {
                pci_address: "0000:01:00.0".to_string(),
                model: "A100".to_string(),
                vendor: "nvidia".to_string(),
                vram_bytes: 80 * 1024 * 1024 * 1024,
                iommu_group: 1,
                numa_node: 0,
            }],
        )
        .await
        .expect("Failed to sync GPUs");

        let pick = |gpu: GpuRequest| {
            let pool = db.pool.clone();
            async move {
                let mut tx = pool.begin().await.expect("begin tx");
                super::pick_host_tx(
                    &mut tx,
                    &SchedulingRequest {
                        memory_bytes: 1024,
                        vcpus: 1,
                        disk_bytes: 100,
                        architecture: None,
                        storage_pool_id: None,
                        required_network_ids: vec![],
                        requires_overlaybd: false,
                        gpu: Some(gpu),
                        placement_policy: None,
                        excluded_host_ids: vec![],
                        required_host_id: None,
                    },
                    &scheduling_config(),
                )
                .await
                .expect("pick host")
                .map(|host| host.id)
            }
        };
        let request = |count, vendor: &str| GpuRequest {
            count,
            vendor: Some(vendor.to_string()),
            model: None,
            min_vram_bytes: None,
        };

        // The GPU host wins despite its higher load.
        assert_eq!(pick(request(1, "nvidia")).await, Some(gpu_host));
        assert_eq!(pick(request(2, "nvidia")).await, None);
        assert_eq!(pick(request(1, "amd")).await, None);
    }
}
