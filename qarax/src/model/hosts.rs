use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction, Type, types::Uuid};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use validator::{Validate, ValidationError, ValidationErrors};

use crate::errors;

/// The version of the control-plane binary, used to detect out-of-date nodes.
pub const CONTROL_PLANE_VERSION: &str = env!("CARGO_PKG_VERSION");

const HOST_COLUMNS: &str = "id, name, address, port, host_user, password, status, cloud_hypervisor_version, firecracker_version, kernel_version, node_version, last_deployed_image, architecture, total_cpus, total_memory_bytes, available_memory_bytes, load_average, disk_total_bytes, disk_available_bytes, resources_updated_at";

/// Build a Host from a sqlx Row containing all host columns.
fn host_from_row(r: &sqlx::postgres::PgRow) -> Host {
    let node_version: Option<String> = r.get("node_version");
    let update_available = node_version
        .as_deref()
        .is_some_and(|v| v != CONTROL_PLANE_VERSION);
    Host {
        id: r.get("id"),
        name: r.get("name"),
        address: r.get("address"),
        port: r.get("port"),
        host_user: r.get("host_user"),
        password: r.get("password"),
        status: r.get("status"),
        cloud_hypervisor_version: r.get("cloud_hypervisor_version"),
        firecracker_version: r.get("firecracker_version"),
        kernel_version: r.get("kernel_version"),
        node_version,
        last_deployed_image: r.get("last_deployed_image"),
        update_available,
        architecture: r.get("architecture"),
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
    pub firecracker_version: Option<String>,
    pub kernel_version: Option<String>,
    /// Version of the qarax-node agent running on this host.
    pub node_version: Option<String>,
    /// Last bootc image deployed to this host via the `/deploy` endpoint.
    pub last_deployed_image: Option<String>,
    /// True when `node_version` differs from the control-plane version.
    pub update_available: bool,
    pub architecture: Option<String>,

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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SchedulingRequest {
    pub memory_bytes: i64,
    pub vcpus: i32,
    pub disk_bytes: i64,
    pub architecture: Option<String>,
    pub storage_pool_id: Option<Uuid>,
    pub required_network_ids: Vec<Uuid>,
    pub gpu: Option<GpuRequest>,
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
}

pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
    architecture_filter: Option<&str>,
) -> Result<Vec<Host>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT {HOST_COLUMNS} FROM hosts WHERE ($1::text IS NULL OR name = $1) AND ($2::text IS NULL OR architecture = $2)"
    ))
    .bind(name_filter)
    .bind(architecture_filter)
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
    let row = sqlx::query(&format!("SELECT {HOST_COLUMNS} FROM hosts WHERE id = $1"))
        .bind(host_id)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|r| host_from_row(&r)))
}

pub async fn pick_host_tx(
    tx: &mut Transaction<'_, Postgres>,
    request: &SchedulingRequest,
    config: &crate::configuration::SchedulingSettings,
) -> Result<Option<Host>, sqlx::Error> {
    let mut qb = QueryBuilder::<Postgres>::new(
        r#"
SELECT h.id, h.name, h.address, h.port, h.host_user, h.password,
       h.status, h.cloud_hypervisor_version, h.firecracker_version, h.kernel_version,
       h.node_version, h.last_deployed_image, h.architecture,
       h.total_cpus, h.total_memory_bytes, h.available_memory_bytes,
       h.load_average, h.disk_total_bytes, h.disk_available_bytes,
       h.resources_updated_at
FROM hosts h
LEFT JOIN (
    SELECT host_id,
           SUM(memory_size)::bigint AS allocated_memory_bytes,
           SUM(boot_vcpus)::bigint AS allocated_vcpus
    FROM vms
    WHERE status NOT IN ('SHUTDOWN', 'UNKNOWN')
      AND host_id IS NOT NULL
    GROUP BY host_id
) alloc ON alloc.host_id = h.id
"#
        .to_string(),
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

    for network_id in &request.required_network_ids {
        qb.push("AND EXISTS (SELECT 1 FROM host_networks hn WHERE hn.host_id = h.id AND hn.network_id = ");
        qb.push_bind(*network_id);
        qb.push(") ");
    }

    if let Some(architecture) = request.architecture.as_deref() {
        qb.push("AND (h.architecture IS NULL OR h.architecture = ");
        qb.push_bind(architecture);
        qb.push(") ");
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

    qb.push("ORDER BY h.load_average ASC NULLS LAST LIMIT 1 FOR UPDATE OF h SKIP LOCKED");

    let row = qb.build().fetch_optional(tx.as_mut()).await?;
    Ok(row.map(|r| host_from_row(&r)))
}

/// Return all UP hosts.
pub async fn list_up(pool: &PgPool) -> Result<Vec<Host>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT {HOST_COLUMNS} FROM hosts WHERE status = 'UP'"
    ))
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| host_from_row(&r)).collect())
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
    sqlx::query(
        "UPDATE hosts SET cloud_hypervisor_version = $1, firecracker_version = $2, kernel_version = $3, node_version = $4 WHERE id = $5",
    )
    .bind(ch_version)
    .bind(fc_version)
    .bind(kernel_version)
    .bind(node_version)
    .bind(id)
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
    sqlx::query("UPDATE hosts SET last_deployed_image = $1 WHERE id = $2")
        .bind(image)
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
    architecture: Option<&str>,
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
    )
    .bind(architecture)
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
    let row = sqlx::query(&format!("SELECT {HOST_COLUMNS} FROM hosts WHERE name = $1"))
        .bind(name)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|r| host_from_row(&r)))
}

pub async fn get_resource_capacity(
    pool: &PgPool,
    host_id: Uuid,
) -> Result<Option<HostResourceCapacity>, sqlx::Error> {
    let row = sqlx::query(
        r#"
SELECT h.id AS host_id,
       h.architecture,
       h.total_cpus,
       COALESCE(alloc.allocated_vcpus, 0)::bigint AS allocated_vcpus,
       h.total_memory_bytes,
       COALESCE(alloc.allocated_memory_bytes, 0)::bigint AS allocated_memory_bytes,
       h.available_memory_bytes,
       h.disk_total_bytes,
       h.disk_available_bytes
FROM hosts h
LEFT JOIN (
    SELECT host_id,
           SUM(memory_size)::bigint AS allocated_memory_bytes,
           SUM(boot_vcpus)::bigint AS allocated_vcpus
    FROM vms
    WHERE status NOT IN ('SHUTDOWN', 'UNKNOWN')
      AND host_id IS NOT NULL
    GROUP BY host_id
) alloc ON alloc.host_id = h.id
WHERE h.id = $1
        "#,
    )
    .bind(host_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| HostResourceCapacity {
        host_id: r.get("host_id"),
        architecture: r.get("architecture"),
        total_cpus: r.get("total_cpus"),
        allocated_vcpus: r.get::<i64, _>("allocated_vcpus"),
        total_memory_bytes: r.get("total_memory_bytes"),
        allocated_memory_bytes: r.get::<i64, _>("allocated_memory_bytes"),
        available_memory_bytes: r.get("available_memory_bytes"),
        disk_total_bytes: r.get("disk_total_bytes"),
        disk_available_bytes: r.get("disk_available_bytes"),
    }))
}

#[cfg(test)]
mod tests {
    use sqlx::{Connection, Executor, PgConnection, PgPool};
    use uuid::Uuid;

    use super::{DeployHostRequest, HostStatus, NewHost, SchedulingRequest};
    use crate::{
        configuration::{SchedulingSettings, get_configuration},
        model::{
            networks::{self, NewNetwork},
            vms::{self, Hypervisor, ResolvedNewVm},
        },
    };

    struct TestDatabase {
        name: String,
        pool: PgPool,
    }

    impl TestDatabase {
        async fn new() -> Self {
            let mut configuration = get_configuration().expect("Failed to read configuration");
            configuration.database.name = Uuid::new_v4().to_string();

            let mut connection =
                PgConnection::connect(&configuration.database.connection_string_without_db())
                    .await
                    .expect("Failed to connect to Postgres");
            connection
                .execute(format!(r#"CREATE DATABASE "{}";"#, configuration.database.name).as_str())
                .await
                .expect("Failed to create test database");

            let pool = PgPool::connect(&configuration.database.connection_string())
                .await
                .expect("Failed to connect to test database");
            sqlx::migrate!("../migrations")
                .run(&pool)
                .await
                .expect("Failed to run migrations");

            Self {
                name: configuration.database.name,
                pool,
            }
        }

        async fn insert_up_host(
            &self,
            name: &str,
            architecture: Option<&str>,
            total_cpus: i32,
            total_memory_bytes: i64,
            disk_available_bytes: i64,
            load_average: f64,
        ) -> Uuid {
            let address_suffix = (name.bytes().fold(0u16, |acc, b| acc + u16::from(b)) % 200) + 1;
            let host_id = super::add(
                &self.pool,
                &NewHost {
                    name: name.to_string(),
                    address: format!("127.0.0.{address_suffix}"),
                    port: 50051,
                    host_user: "root".to_string(),
                    password: String::new(),
                },
            )
            .await
            .expect("Failed to insert host");

            super::update_status(&self.pool, host_id, HostStatus::Up)
                .await
                .expect("Failed to mark host UP");
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
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let db_name = self.name.clone();
            let (tx, rx) = std::sync::mpsc::channel();

            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                rt.block_on(async move {
                    let configuration = get_configuration().expect("Failed to read configuration");
                    let mut connection =
                        PgConnection::connect_with(&configuration.database.without_db())
                            .await
                            .expect("Failed to connect to Postgres");
                    connection
                        .execute(&*format!(r#"DROP DATABASE "{}" WITH (FORCE)"#, db_name))
                        .await
                        .expect("Failed to drop test database");
                    let _ = tx.send(());
                });
            });

            let _ = rx.recv();
        }
    }

    fn scheduling_config() -> SchedulingSettings {
        SchedulingSettings {
            memory_oversubscription_ratio: 1.0,
            cpu_oversubscription_ratio: 1.0,
            disk_headroom_bytes: 10,
            memory_health_floor_bytes: 0,
        }
    }

    fn resolved_vm(name: &str, memory_size: i64, boot_vcpus: i32) -> ResolvedNewVm {
        ResolvedNewVm {
            name: name.to_string(),
            tags: vec![],
            hypervisor: Hypervisor::CloudHv,
            architecture: None,
            boot_vcpus,
            max_vcpus: boot_vcpus,
            cpu_topology: None,
            kvm_hyperv: None,
            memory_size,
            memory_hotplug_size: None,
            memory_mergeable: None,
            memory_shared: None,
            memory_hugepages: None,
            memory_hugepage_size: None,
            memory_prefault: None,
            memory_thp: None,
            boot_source_id: None,
            root_disk_object_id: None,
            boot_mode: None,
            description: None,
            image_ref: None,
            cloud_init_user_data: None,
            cloud_init_meta_data: None,
            cloud_init_network_config: None,
            network_id: None,
            networks: None,
            security_group_ids: None,
            accelerator_config: None,
            numa_config: None,
            persistent_upper_pool_id: None,
            config: serde_json::json!({}),
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
                gpu: None,
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
                gpu: None,
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
            gpu: None,
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

        let capacity = super::get_resource_capacity(&db.pool, host_id)
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
        networks::attach_host(&db.pool, network_id, attached_host_id, "qtnet0")
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
                gpu: None,
            },
            &scheduling_config(),
        )
        .await
        .expect("pick host")
        .expect("expected host");

        assert_eq!(host.id, attached_host_id);
        assert_ne!(host.id, unattached_host_id);
    }
}
