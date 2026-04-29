/// Background task that periodically polls resource metrics from UP hosts
/// and persists them in the database for resource-aware scheduling.
use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;
use tokio::time::{Duration, Instant, interval_at};
use tracing::warn;

use crate::grpc_client::{NodeClient, node::NodeInfo};
use crate::model::host_gpus::{self, GpuDiscovery};
use crate::model::host_numa::{self, NumaNodeDiscovery};
use crate::model::hosts::{self, Host, HostStatus};

async fn handle_probe_result(pool: &PgPool, host: &Host, node_info: Result<NodeInfo>) {
    match node_info {
        Ok(info) => {
            if let Err(e) = hosts::update_versions(
                pool,
                host.id,
                &info.cloud_hypervisor_version,
                info.firecracker_version.as_deref(),
                &info.kernel_version,
                &info.node_version,
            )
            .await
            {
                warn!(
                    "Resource monitor: failed to update versions for host {}: {}",
                    host.name, e
                );
            }

            if let Err(e) = hosts::update_resources(
                pool,
                host.id,
                Some(&info.architecture),
                info.total_cpus,
                info.total_memory_bytes,
                info.available_memory_bytes,
                info.load_average_1m,
                info.disk_total_bytes,
                info.disk_available_bytes,
            )
            .await
            {
                warn!(
                    "Resource monitor: failed to update resources for host {}: {}",
                    host.name, e
                );
            }

            // Sync GPU inventory
            let gpu_discoveries: Vec<GpuDiscovery> = info
                .gpus
                .iter()
                .map(|g| GpuDiscovery {
                    pci_address: g.pci_address.clone(),
                    model: g.model.clone(),
                    vendor: g.vendor.clone(),
                    vram_bytes: g.vram_bytes,
                    iommu_group: g.iommu_group,
                    numa_node: g.numa_node,
                })
                .collect();
            if let Err(e) = host_gpus::sync_gpus(pool, host.id, &gpu_discoveries).await {
                warn!(
                    "Resource monitor: failed to sync GPUs for host {}: {}",
                    host.name, e
                );
            }

            // Sync NUMA topology
            let numa_discoveries: Vec<NumaNodeDiscovery> = info
                .numa_nodes
                .iter()
                .map(|n| NumaNodeDiscovery {
                    node_id: n.id,
                    cpu_list: host_numa::expand_cpu_list_to_string(&n.cpus),
                    memory_bytes: if n.memory_bytes > 0 {
                        Some(n.memory_bytes)
                    } else {
                        None
                    },
                    distances: n.distances.clone(),
                })
                .collect();
            if let Err(e) = host_numa::sync_numa_nodes(pool, host.id, &numa_discoveries).await {
                warn!(
                    "Resource monitor: failed to sync NUMA topology for host {}: {}",
                    host.name, e
                );
            }
        }
        Err(e) => {
            warn!(
                "Resource monitor: failed to get node info from host {}: {}",
                host.name, e
            );

            if host.status == HostStatus::Maintenance {
                return;
            }

            if let Err(update_error) = hosts::update_status(pool, host.id, HostStatus::Down).await {
                warn!(
                    "Resource monitor: failed to mark host {} DOWN after probe failure: {}",
                    host.name, update_error
                );
            }
        }
    }
}

pub async fn start_resource_monitor(pool: Arc<PgPool>) {
    let period = Duration::from_secs(30);
    let mut ticker = interval_at(Instant::now() + period, period);

    loop {
        ticker.tick().await;

        #[cfg(feature = "otel")]
        let _cycle_start = std::time::Instant::now();

        let up_hosts = match hosts::list_probeable(&pool).await {
            Ok(h) => h,
            Err(e) => {
                warn!("Resource monitor: failed to list probeable hosts: {}", e);
                continue;
            }
        };

        if up_hosts.is_empty() {
            continue;
        }

        for host in up_hosts {
            let client = NodeClient::new(&host.address, host.port as u16);
            handle_probe_result(&pool, &host, client.get_node_info().await).await;
        }

        #[cfg(feature = "otel")]
        crate::vm_monitor::record_monitor_cycle("resource", _cycle_start);
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use sqlx::{Connection, Executor, PgConnection, PgPool};
    use uuid::Uuid;

    use super::handle_probe_result;
    use crate::{
        configuration::get_configuration,
        grpc_client::node::NodeInfo,
        model::hosts::{self, HostStatus, NewHost},
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

        async fn insert_up_host(&self) -> hosts::Host {
            let host_id = hosts::add(
                &self.pool,
                &NewHost {
                    name: "test-host".to_string(),
                    address: "127.0.0.1".to_string(),
                    port: 50051,
                    host_user: "root".to_string(),
                    password: String::new(),
                },
            )
            .await
            .expect("Failed to insert host");

            hosts::update_status(&self.pool, host_id, HostStatus::Up)
                .await
                .expect("Failed to mark host UP");

            hosts::get_by_id(&self.pool, host_id)
                .await
                .expect("Failed to fetch host")
                .expect("Host not found")
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

    #[tokio::test]
    async fn probe_failure_marks_host_down() {
        let db = TestDatabase::new().await;
        let host = db.insert_up_host().await;

        handle_probe_result(&db.pool, &host, Err(anyhow!("host unreachable"))).await;

        let updated = hosts::get_by_id(&db.pool, host.id)
            .await
            .expect("Failed to fetch updated host")
            .expect("Updated host not found");
        assert_eq!(updated.status, HostStatus::Down);
    }

    #[tokio::test]
    async fn probe_failure_keeps_maintenance_host_in_maintenance() {
        let db = TestDatabase::new().await;
        let host = db.insert_up_host().await;
        hosts::update_status(&db.pool, host.id, HostStatus::Maintenance)
            .await
            .expect("mark maintenance");
        let host = hosts::get_by_id(&db.pool, host.id)
            .await
            .expect("fetch maintenance host")
            .expect("maintenance host exists");

        handle_probe_result(&db.pool, &host, Err(anyhow!("host unreachable"))).await;

        let updated = hosts::get_by_id(&db.pool, host.id)
            .await
            .expect("Failed to fetch updated host")
            .expect("Updated host not found");
        assert_eq!(updated.status, HostStatus::Maintenance);
    }

    #[tokio::test]
    async fn successful_probe_keeps_host_up_and_updates_resources() {
        let db = TestDatabase::new().await;
        let host = db.insert_up_host().await;

        handle_probe_result(
            &db.pool,
            &host,
            Ok(NodeInfo {
                hostname: "node-1".to_string(),
                cloud_hypervisor_version: "44.0".to_string(),
                firecracker_version: Some("Firecracker v1.11.0".to_string()),
                kernel_version: "6.12.0".to_string(),
                node_version: "0.1.0".to_string(),
                total_cpus: 8,
                total_memory_bytes: 16 * 1024 * 1024,
                available_memory_bytes: 8 * 1024 * 1024,
                load_average_1m: 0.5,
                disk_total_bytes: 100 * 1024 * 1024,
                disk_available_bytes: 60 * 1024 * 1024,
                gpus: vec![],
                numa_nodes: vec![],
                architecture: "x86_64".to_string(),
            }),
        )
        .await;

        let updated = hosts::get_by_id(&db.pool, host.id)
            .await
            .expect("Failed to fetch updated host")
            .expect("Updated host not found");
        assert_eq!(updated.status, HostStatus::Up);
        assert_eq!(updated.architecture.as_deref(), Some("x86_64"));
        assert_eq!(updated.total_cpus, Some(8));
        assert_eq!(updated.available_memory_bytes, Some(8 * 1024 * 1024));
        assert_eq!(
            updated.firecracker_version.as_deref(),
            Some("Firecracker v1.11.0")
        );
    }
}
