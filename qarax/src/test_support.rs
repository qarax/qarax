//! Fixtures for the crate's database-backed unit tests.

use std::sync::atomic::{AtomicU8, Ordering};

use sqlx::{Connection, Executor, PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    App,
    configuration::{default_control_plane_architecture, get_configuration},
    model::{
        hosts::{self, HostStatus, NewHost},
        vms::{Hypervisor, ResolvedNewVm},
    },
};

/// A fresh, migrated database, dropped when the value is.
pub(crate) struct TestDatabase {
    name: String,
    pub(crate) pool: PgPool,
}

impl TestDatabase {
    pub(crate) async fn new() -> Self {
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

    pub(crate) fn app(&self) -> App {
        let mut configuration = get_configuration().expect("Failed to read configuration");
        configuration.database.name = self.name.clone();
        App::new(
            self.pool.clone(),
            configuration.database,
            configuration.vm_defaults,
            configuration.scheduling,
            configuration.sandbox,
            configuration.auth,
            default_control_plane_architecture(),
        )
    }

    /// Adds a host in `status`. Each host gets its own address, since
    /// `hosts.address` is unique.
    pub(crate) async fn add_host(&self, name: &str, status: HostStatus) -> Uuid {
        static NEXT_ADDRESS: AtomicU8 = AtomicU8::new(1);
        let host_id = hosts::add(
            &self.pool,
            &NewHost {
                name: name.to_string(),
                address: format!("127.0.0.{}", NEXT_ADDRESS.fetch_add(1, Ordering::Relaxed)),
                port: 50051,
                host_user: "root".to_string(),
                credential_ref: None,
                reservation_class: None,
                placement_labels: Default::default(),
            },
        )
        .await
        .expect("Failed to insert host");
        hosts::transition(&self.pool, host_id, &[HostStatus::Down], status)
            .await
            .expect("Failed to set host status");
        host_id
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let db_name = self.name.clone();
        let _ = std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .expect("Failed to create runtime")
                .block_on(async move {
                    let configuration = get_configuration().expect("Failed to read configuration");
                    let mut connection =
                        PgConnection::connect_with(&configuration.database.without_db())
                            .await
                            .expect("Failed to connect to Postgres");
                    connection
                        .execute(format!(r#"DROP DATABASE "{db_name}" WITH (FORCE)"#).as_str())
                        .await
                        .expect("Failed to drop test database");
                });
        })
        .join();
    }
}

/// A one-vCPU, 256 MiB Cloud Hypervisor VM with every optional field unset.
pub(crate) fn resolved_vm(name: &str) -> ResolvedNewVm {
    ResolvedNewVm {
        name: name.to_string(),
        tags: vec![],
        hypervisor: Hypervisor::CloudHv,
        architecture: None,
        boot_vcpus: 1,
        max_vcpus: 1,
        cpu_topology: None,
        kvm_hyperv: None,
        memory_size: 256 * 1024 * 1024,
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
        placement_policy: None,
        config: serde_json::json!({}),
    }
}
