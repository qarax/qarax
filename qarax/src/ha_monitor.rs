/// Background task that automatically fails over HA-enabled VMs from failed
/// hosts.
///
/// The resource monitor marks hosts `Down` when probes fail. This monitor
/// re-probes those hosts on every tick and, once a host has stayed
/// unreachable for at least `ha.failover_grace_seconds`, cold-restarts its
/// resident VMs that have `ha_enabled` on another eligible host via the
/// regular scheduling path.
///
/// Split-brain guards (a VM must never run twice against shared storage):
/// - failover only happens after the grace period *and* a fresh probe failure
///   immediately before acting,
/// - only VMs whose disks all live on shared pools are eligible,
/// - failed-over VMs are tagged with `failed_over_from` in their config; if
///   the source host ever becomes reachable again while still marked Down,
///   the stale copies are force-stopped and deleted before anything else.
use std::collections::{HashMap, HashSet};

use tokio::time::{Duration, Instant, interval_at};
use tracing::{info, warn};
use uuid::Uuid;

use crate::App;
use crate::configuration::HaSettings;
use crate::errors::Error;
use crate::grpc_client::NodeClient;
use crate::handlers::host::handler::evacuation_scheduling_request;
use crate::handlers::vm::handler::start_vm_internal;
use crate::model::{
    host_gpus,
    hosts::{self, Host},
    jobs::{self, JobType, NewJob},
    storage_objects, storage_pools, vm_disks,
    vms::{self, Vm, VmStatus},
};

#[derive(Default)]
struct MonitorState {
    /// When the monitor itself first confirmed each down host unreachable.
    unreachable_since: HashMap<Uuid, Instant>,
    /// Last failover attempt per VM, for retry cooldown.
    last_attempt: HashMap<Uuid, Instant>,
}

pub async fn start_ha_monitor(env: App, settings: HaSettings) {
    if !settings.enabled {
        info!("HA monitor: disabled by configuration");
        return;
    }

    let period = Duration::from_secs(settings.check_interval_seconds.max(1));
    let mut ticker = interval_at(Instant::now() + period, period);
    let mut state = MonitorState::default();

    loop {
        ticker.tick().await;

        if env.maintenance_mode() {
            continue;
        }

        #[cfg(feature = "otel")]
        let _cycle_start = std::time::Instant::now();

        run_cycle(&env, &settings, &mut state).await;

        #[cfg(feature = "otel")]
        crate::vm_monitor::record_monitor_cycle("ha", _cycle_start);
    }
}

async fn run_cycle(env: &App, settings: &HaSettings, state: &mut MonitorState) {
    let down_hosts = match hosts::list_down(env.pool()).await {
        Ok(hosts) => hosts,
        Err(e) => {
            warn!("HA monitor: failed to list down hosts: {}", e);
            return;
        }
    };

    let down_ids: HashSet<Uuid> = down_hosts.iter().map(|h| h.id).collect();
    state
        .unreachable_since
        .retain(|host_id, _| down_ids.contains(host_id));

    for host in down_hosts {
        let client = NodeClient::new(&host.address, host.port as u16);
        let is_reachable = tokio::time::timeout(Duration::from_secs(5), client.get_node_info())
            .await
            .map(|res| res.is_ok())
            .unwrap_or(false);
        if is_reachable {
            // The host is reachable despite being marked Down (e.g. it
            // recovered before an operator re-enabled it). Never fail over
            // from a live host; instead remove stale copies of VMs that were
            // already failed over from it.
            state.unreachable_since.remove(&host.id);
            cleanup_stale_copies(env, &host).await;
            continue;
        }

        let first_unreachable = *state
            .unreachable_since
            .entry(host.id)
            .or_insert_with(Instant::now);
        if first_unreachable.elapsed() < Duration::from_secs(settings.failover_grace_seconds) {
            continue;
        }

        failover_host(env, settings, state, &host).await;
    }
}

/// Remove leftover VM processes on a recovered host for VMs that were failed
/// over to another host, then clear their `failed_over_from` marker.
async fn cleanup_stale_copies(env: &App, host: &Host) {
    let stale_vms = match vms::list_failed_over_from(env.pool(), host.id).await {
        Ok(vms) => vms,
        Err(e) => {
            warn!(
                "HA monitor: failed to list failed-over VMs for host {}: {}",
                host.name, e
            );
            return;
        }
    };

    if stale_vms.is_empty() {
        return;
    }

    let client = NodeClient::new(&host.address, host.port as u16);
    for vm in stale_vms {
        let _ = client.force_stop_vm(vm.id).await;
        if let Err(e) = client.delete_vm(vm.id).await {
            let not_found = e
                .downcast_ref::<Error>()
                .map(|e| matches!(e, Error::NotFound))
                .unwrap_or(false);
            if !not_found {
                warn!(
                    "HA monitor: failed to delete stale copy of VM {} on host {}: {}",
                    vm.id, host.name, e
                );
                continue;
            }
        }

        info!(
            "HA monitor: removed stale copy of VM {} from recovered host {}",
            vm.id, host.name
        );
        let mut config = vm.config.clone();
        vms::set_failed_over_from(&mut config, None);
        if let Err(e) = vms::update_config(env.pool(), vm.id, &config).await {
            warn!(
                "HA monitor: failed to clear failed_over_from for VM {}: {}",
                vm.id, e
            );
        }
    }
}

/// VMs on the given host that are eligible for automatic failover.
async fn collect_failover_candidates(env: &App, host_id: Uuid) -> Result<Vec<Vm>, sqlx::Error> {
    let resident = vms::list_by_host(env.pool(), host_id).await?;
    Ok(resident
        .into_iter()
        .filter(|vm| {
            vm.ha_enabled
                && matches!(
                    vm.status,
                    VmStatus::Running | VmStatus::Paused | VmStatus::Unknown
                )
        })
        .collect())
}

async fn failover_host(env: &App, settings: &HaSettings, state: &mut MonitorState, host: &Host) {
    let candidates = match collect_failover_candidates(env, host.id).await {
        Ok(vms) => vms,
        Err(e) => {
            warn!(
                "HA monitor: failed to list resident VMs for host {}: {}",
                host.name, e
            );
            return;
        }
    };

    let cooldown = Duration::from_secs(settings.retry_cooldown_seconds);
    for vm in candidates {
        if state
            .last_attempt
            .get(&vm.id)
            .is_some_and(|attempted| attempted.elapsed() < cooldown)
        {
            continue;
        }
        state.last_attempt.insert(vm.id, Instant::now());
        failover_vm(env, &vm, host).await;
    }
}

async fn failover_vm(env: &App, vm: &Vm, dead_host: &Host) {
    info!(
        "HA monitor: failing over VM {} ({}) from dead host {} ({})",
        vm.name, vm.id, dead_host.name, dead_host.id
    );

    let job = match jobs::create(
        env.pool(),
        NewJob {
            job_type: JobType::VmFailover,
            description: Some(format!(
                "Failing over VM {} from host {}",
                vm.name, dead_host.name
            )),
            resource_id: Some(vm.id),
            resource_type: Some(jobs::resource_types::VM.to_string()),
        },
    )
    .await
    {
        Ok(job) => job,
        Err(e) => {
            warn!(
                "HA monitor: failed to create failover job for VM {}: {}",
                vm.id, e
            );
            return;
        }
    };

    if let Err(e) = jobs::mark_running(env.pool(), job.id).await {
        warn!(
            "HA monitor: failed to mark failover job {} as running: {}",
            job.id, e
        );
    }

    match execute_failover(env, vm, dead_host).await {
        Ok(result) => {
            let _ = jobs::mark_completed(env.pool(), job.id, Some(result)).await;
        }
        Err(error) => {
            warn!(
                "HA monitor: failover of VM {} from host {} failed: {}",
                vm.name, dead_host.name, error
            );
            let _ = jobs::mark_failed(env.pool(), job.id, &error.to_string()).await;
        }
    }
}

async fn execute_failover(
    env: &App,
    vm: &Vm,
    dead_host: &Host,
) -> Result<serde_json::Value, Error> {
    let required_pool_ids = shared_disk_pool_ids(env, vm).await?;
    let gpu_request = gpu_request_for_vm(env, vm).await?;
    let target =
        pick_failover_host(env, vm, dead_host.id, &required_pool_ids, &gpu_request).await?;

    if let Some(gpu_request) = &gpu_request {
        reallocate_vm_gpus(env, vm.id, target.id, gpu_request).await?;
    }

    // Best-effort: if the dead host is partially alive, remove the old copy
    // before starting elsewhere. Use a short timeout since the host is
    // already known to be unreachable.
    let old_client = NodeClient::new(&dead_host.address, dead_host.port as u16);
    let cleanup_timeout = Duration::from_secs(2);
    let _ = tokio::time::timeout(cleanup_timeout, old_client.force_stop_vm(vm.id)).await;
    let _ = tokio::time::timeout(cleanup_timeout, old_client.delete_vm(vm.id)).await;

    // Tag the VM so the stale copy is cleaned up if the source host recovers.
    let mut config = vm.config.clone();
    vms::set_failed_over_from(&mut config, Some(dead_host.id));
    vms::update_config(env.pool(), vm.id, &config).await?;

    vms::update_host_id(env.pool(), vm.id, target.id).await?;
    // Created makes the start path re-create the VM on the new host.
    vms::update_status(env.pool(), vm.id, VmStatus::Created).await?;

    let start_job_id = match start_vm_internal(env, vm.id).await {
        Ok(job_id) => job_id,
        Err(e) => {
            // Revert so the HA monitor picks this VM back up as a candidate
            // and retries on a subsequent tick, instead of leaving it
            // stranded in `Created` on the target host.
            let _ = vms::update_host_id(env.pool(), vm.id, dead_host.id).await;
            let _ = vms::update_status(env.pool(), vm.id, vm.status).await;
            let _ = vms::update_config(env.pool(), vm.id, &vm.config).await;
            return Err(e);
        }
    };

    info!(
        "HA monitor: VM {} ({}) failed over from host {} to host {} (start job {})",
        vm.name, vm.id, dead_host.name, target.name, start_job_id
    );

    Ok(serde_json::json!({
        "source_host_id": dead_host.id,
        "target_host_id": target.id,
        "start_job_id": start_job_id,
    }))
}

/// GPU requirements derived from the GPUs currently allocated to the VM on
/// the dead host. `None` means the VM has no GPUs attached.
async fn gpu_request_for_vm(env: &App, vm: &Vm) -> Result<Option<hosts::GpuRequest>, Error> {
    let allocated = host_gpus::list_by_vm(env.pool(), vm.id).await?;
    if allocated.is_empty() {
        return Ok(None);
    }

    Ok(Some(hosts::GpuRequest {
        count: allocated.len() as i32,
        vendor: allocated[0].vendor.clone(),
        model: allocated[0].model.clone(),
        min_vram_bytes: allocated.iter().filter_map(|g| g.vram_bytes).min(),
    }))
}

/// Move the VM's GPU allocation from the dead host to `target_host_id`,
/// atomically within a transaction so a partial allocation never leaves the
/// VM without its required GPUs.
async fn reallocate_vm_gpus(
    env: &App,
    vm_id: Uuid,
    target_host_id: Uuid,
    gpu_request: &hosts::GpuRequest,
) -> Result<(), Error> {
    let mut tx = env.pool().begin().await?;
    host_gpus::deallocate_by_vm_tx(&mut tx, vm_id).await?;
    let allocated = host_gpus::allocate_gpus(
        &mut tx,
        target_host_id,
        vm_id,
        gpu_request.count,
        gpu_request.vendor.as_deref(),
        gpu_request.model.as_deref(),
        gpu_request.min_vram_bytes,
    )
    .await?;

    if (allocated.len() as i32) < gpu_request.count {
        return Err(Error::UnprocessableEntity(format!(
            "requested {} GPUs for failover but only {} available on target host",
            gpu_request.count,
            allocated.len()
        )));
    }

    tx.commit().await?;
    Ok(())
}

/// Pool IDs backing the VM's disks (including persistent OverlayBD upper
/// layers). Errors if any of them is not on shared storage — such disks
/// cannot be reattached on another host.
async fn shared_disk_pool_ids(env: &App, vm: &Vm) -> Result<Vec<Uuid>, Error> {
    let db_disks = vm_disks::list_by_vm(env.pool(), vm.id).await?;
    let so_ids: Vec<Uuid> = db_disks
        .iter()
        .filter_map(|d| d.storage_object_id)
        .chain(db_disks.iter().filter_map(|d| d.upper_storage_object_id))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if so_ids.is_empty() {
        return Ok(Vec::new());
    }

    let objects = storage_objects::get_batch(env.pool(), &so_ids).await?;
    let pool_ids: Vec<Uuid> = objects
        .iter()
        .map(|o| o.storage_pool_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let pools = storage_pools::get_batch(env.pool(), &pool_ids).await?;
    for pool in &pools {
        if !pool.pool_type.is_shared() {
            return Err(Error::UnprocessableEntity(format!(
                "disk on pool '{}' ({:?}) is not on shared storage; cannot fail over",
                pool.name, pool.pool_type
            )));
        }
    }

    Ok(pool_ids)
}

/// Pick a destination host for the VM, excluding the dead host and any
/// candidate that does not have all required storage pools attached.
async fn pick_failover_host(
    env: &App,
    vm: &Vm,
    dead_host_id: Uuid,
    required_pool_ids: &[Uuid],
    gpu_request: &Option<hosts::GpuRequest>,
) -> Result<Host, Error> {
    let base_request =
        evacuation_scheduling_request(env, vm, dead_host_id, gpu_request.clone()).await?;
    let mut excluded_host_ids = base_request.excluded_host_ids.clone();

    loop {
        let mut tx = env.pool().begin().await?;
        let candidate = hosts::pick_host_tx(
            &mut tx,
            &hosts::SchedulingRequest {
                excluded_host_ids: excluded_host_ids.clone(),
                ..base_request.clone()
            },
            env.scheduling(),
        )
        .await?;
        drop(tx);

        let Some(candidate) = candidate else {
            return Err(Error::UnprocessableEntity(format!(
                "no eligible destination host found for VM '{}'",
                vm.name
            )));
        };

        let mut has_all_pools = true;
        for pool_id in required_pool_ids {
            if !storage_pools::host_has_pool(env.pool(), candidate.id, *pool_id).await? {
                has_all_pools = false;
                break;
            }
        }
        if has_all_pools {
            return Ok(candidate);
        }
        excluded_host_ids.push(candidate.id);
    }
}

#[cfg(test)]
mod tests {
    use sqlx::{Connection, Executor, PgConnection, PgPool};
    use uuid::Uuid;

    use super::{collect_failover_candidates, gpu_request_for_vm, reallocate_vm_gpus};
    use crate::{
        App,
        configuration::{default_control_plane_architecture, get_configuration},
        model::{
            host_gpus::{self, GpuDiscovery},
            hosts::{self, GpuRequest, HostStatus, NewHost},
            vms::{self, BootMode, Hypervisor, ResolvedNewVm, VmStatus},
        },
    };

    #[test]
    fn ha_enabled_defaults_to_false() {
        assert!(!vms::ha_enabled(&serde_json::json!({})));
        assert!(!vms::ha_enabled(&serde_json::Value::Null));
        assert!(vms::ha_enabled(&serde_json::json!({"ha_enabled": true})));
    }

    #[test]
    fn set_ha_enabled_config_round_trips() {
        let mut config = serde_json::Value::Null;
        vms::set_ha_enabled_config(&mut config, true);
        assert!(vms::ha_enabled(&config));
        vms::set_ha_enabled_config(&mut config, false);
        assert!(!vms::ha_enabled(&config));
    }

    #[test]
    fn failed_over_from_round_trips() {
        let host_id = Uuid::new_v4();
        let mut config = serde_json::json!({"ha_enabled": true});

        vms::set_failed_over_from(&mut config, Some(host_id));
        assert_eq!(vms::failed_over_from(&config), Some(host_id));
        // The HA flag must survive the marker updates.
        assert!(vms::ha_enabled(&config));

        vms::set_failed_over_from(&mut config, None);
        assert_eq!(vms::failed_over_from(&config), None);
        assert!(vms::ha_enabled(&config));
    }

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

        fn app(&self) -> App {
            let mut configuration = get_configuration().expect("Failed to read configuration");
            configuration.database.name = self.name.clone();
            App::new(
                self.pool.clone(),
                configuration.database,
                configuration.vm_defaults,
                configuration.scheduling,
                configuration.sandbox,
                default_control_plane_architecture(),
            )
        }

        async fn insert_down_host(&self) -> Uuid {
            self.insert_down_host_with_address("ha-test-host", "127.0.0.1")
                .await
        }

        async fn insert_down_host_with_address(&self, name: &str, address: &str) -> Uuid {
            let host_id = hosts::add(
                &self.pool,
                &NewHost {
                    name: name.to_string(),
                    address: address.to_string(),
                    port: 50051,
                    host_user: "root".to_string(),
                    password: String::new(),
                    reservation_class: None,
                    placement_labels: std::collections::BTreeMap::new(),
                },
            )
            .await
            .expect("Failed to insert host");

            hosts::update_status(&self.pool, host_id, HostStatus::Down)
                .await
                .expect("Failed to mark host DOWN");
            host_id
        }

        async fn insert_vm(&self, name: &str, host_id: Uuid, status: VmStatus, ha: bool) -> Uuid {
            let resolved = ResolvedNewVm {
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
                boot_mode: Some(BootMode::Kernel),
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
                config: serde_json::json!({ "ha_enabled": ha }),
            };
            let vm_id = vms::create(&self.pool, &resolved)
                .await
                .expect("Failed to insert VM");
            vms::update_host_id(&self.pool, vm_id, host_id)
                .await
                .expect("Failed to assign VM host");
            vms::update_status(&self.pool, vm_id, status)
                .await
                .expect("Failed to set VM status");
            vm_id
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
    async fn only_running_ha_vms_are_failover_candidates() {
        let db = TestDatabase::new().await;
        let host_id = db.insert_down_host().await;

        let ha_running = db
            .insert_vm("ha-running", host_id, VmStatus::Running, true)
            .await;
        let ha_unknown = db
            .insert_vm("ha-unknown", host_id, VmStatus::Unknown, true)
            .await;
        let _non_ha_running = db
            .insert_vm("plain-running", host_id, VmStatus::Running, false)
            .await;
        let _ha_stopped = db
            .insert_vm("ha-stopped", host_id, VmStatus::Shutdown, true)
            .await;

        let candidates = collect_failover_candidates(&db.app(), host_id)
            .await
            .expect("Failed to collect candidates");

        let mut ids: Vec<Uuid> = candidates.iter().map(|vm| vm.id).collect();
        ids.sort();
        let mut expected = vec![ha_running, ha_unknown];
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[tokio::test]
    async fn failed_over_vms_are_listed_by_source_host() {
        let db = TestDatabase::new().await;
        let host_id = db.insert_down_host().await;
        let vm_id = db
            .insert_vm("ha-failed-over", host_id, VmStatus::Running, true)
            .await;

        let vm = vms::get(&db.pool, vm_id).await.expect("Failed to get VM");
        let mut config = vm.config.clone();
        vms::set_failed_over_from(&mut config, Some(host_id));
        vms::update_config(&db.pool, vm_id, &config)
            .await
            .expect("Failed to update VM config");

        let listed = vms::list_failed_over_from(&db.pool, host_id)
            .await
            .expect("Failed to list failed-over VMs");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, vm_id);

        vms::set_failed_over_from(&mut config, None);
        vms::update_config(&db.pool, vm_id, &config)
            .await
            .expect("Failed to clear marker");

        let listed = vms::list_failed_over_from(&db.pool, host_id)
            .await
            .expect("Failed to list failed-over VMs");
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn gpu_request_reflects_current_allocation() {
        let db = TestDatabase::new().await;
        let host_id = db.insert_down_host().await;
        let vm_id = db
            .insert_vm("gpu-vm", host_id, VmStatus::Running, true)
            .await;

        host_gpus::sync_gpus(
            &db.pool,
            host_id,
            &[GpuDiscovery {
                pci_address: "0000:00:01.0".to_string(),
                model: "A100".to_string(),
                vendor: "nvidia".to_string(),
                vram_bytes: 80 * 1024 * 1024 * 1024,
                iommu_group: 1,
                numa_node: 0,
            }],
        )
        .await
        .expect("Failed to sync GPUs");

        let mut tx = db.pool.begin().await.expect("Failed to begin tx");
        let allocated = host_gpus::allocate_gpus(&mut tx, host_id, vm_id, 1, None, None, None)
            .await
            .expect("Failed to allocate GPU");
        assert_eq!(allocated.len(), 1);
        tx.commit().await.expect("Failed to commit");

        let vm = vms::get(&db.pool, vm_id).await.expect("Failed to get VM");
        let gpu_request = gpu_request_for_vm(&db.app(), &vm)
            .await
            .expect("Failed to compute GPU request")
            .expect("Expected a GPU request");

        assert_eq!(gpu_request.count, 1);
        assert_eq!(gpu_request.vendor, Some("nvidia".to_string()));
        assert_eq!(gpu_request.model, Some("A100".to_string()));
        assert_eq!(gpu_request.min_vram_bytes, Some(80 * 1024 * 1024 * 1024));
    }

    #[tokio::test]
    async fn gpu_request_is_none_without_allocated_gpus() {
        let db = TestDatabase::new().await;
        let host_id = db.insert_down_host().await;
        let vm_id = db
            .insert_vm("no-gpu-vm", host_id, VmStatus::Running, true)
            .await;

        let vm = vms::get(&db.pool, vm_id).await.expect("Failed to get VM");
        let gpu_request = gpu_request_for_vm(&db.app(), &vm)
            .await
            .expect("Failed to compute GPU request");
        assert!(gpu_request.is_none());
    }

    #[tokio::test]
    async fn reallocate_vm_gpus_moves_allocation_to_target_host() {
        let db = TestDatabase::new().await;
        let dead_host_id = db
            .insert_down_host_with_address("ha-test-host-dead", "127.0.0.1")
            .await;
        let target_host_id = db
            .insert_down_host_with_address("ha-test-host-target", "127.0.0.2")
            .await;
        let vm_id = db
            .insert_vm("gpu-vm", dead_host_id, VmStatus::Running, true)
            .await;

        host_gpus::sync_gpus(
            &db.pool,
            dead_host_id,
            &[GpuDiscovery {
                pci_address: "0000:00:01.0".to_string(),
                model: "A100".to_string(),
                vendor: "nvidia".to_string(),
                vram_bytes: 80 * 1024 * 1024 * 1024,
                iommu_group: 1,
                numa_node: 0,
            }],
        )
        .await
        .expect("Failed to sync GPUs on dead host");

        host_gpus::sync_gpus(
            &db.pool,
            target_host_id,
            &[GpuDiscovery {
                pci_address: "0000:00:02.0".to_string(),
                model: "A100".to_string(),
                vendor: "nvidia".to_string(),
                vram_bytes: 80 * 1024 * 1024 * 1024,
                iommu_group: 1,
                numa_node: 0,
            }],
        )
        .await
        .expect("Failed to sync GPUs on target host");

        let mut tx = db.pool.begin().await.expect("Failed to begin tx");
        host_gpus::allocate_gpus(&mut tx, dead_host_id, vm_id, 1, None, None, None)
            .await
            .expect("Failed to allocate GPU on dead host");
        tx.commit().await.expect("Failed to commit");

        let gpu_request = GpuRequest {
            count: 1,
            vendor: Some("nvidia".to_string()),
            model: Some("A100".to_string()),
            min_vram_bytes: None,
        };
        reallocate_vm_gpus(&db.app(), vm_id, target_host_id, &gpu_request)
            .await
            .expect("Failed to reallocate GPUs");

        let allocated = host_gpus::list_by_vm(&db.pool, vm_id)
            .await
            .expect("Failed to list GPUs by VM");
        assert_eq!(allocated.len(), 1);
        assert_eq!(allocated[0].host_id, target_host_id);
        assert_eq!(allocated[0].pci_address, "0000:00:02.0");

        let dead_host_gpus = host_gpus::list_by_host(&db.pool, dead_host_id)
            .await
            .expect("Failed to list GPUs on dead host");
        assert!(dead_host_gpus[0].vm_id.is_none());
    }

    #[tokio::test]
    async fn reallocate_vm_gpus_fails_when_target_lacks_gpus() {
        let db = TestDatabase::new().await;
        let dead_host_id = db
            .insert_down_host_with_address("ha-test-host-dead", "127.0.0.1")
            .await;
        let target_host_id = db
            .insert_down_host_with_address("ha-test-host-target", "127.0.0.2")
            .await;
        let vm_id = db
            .insert_vm("gpu-vm", dead_host_id, VmStatus::Running, true)
            .await;

        host_gpus::sync_gpus(
            &db.pool,
            dead_host_id,
            &[GpuDiscovery {
                pci_address: "0000:00:01.0".to_string(),
                model: "A100".to_string(),
                vendor: "nvidia".to_string(),
                vram_bytes: 80 * 1024 * 1024 * 1024,
                iommu_group: 1,
                numa_node: 0,
            }],
        )
        .await
        .expect("Failed to sync GPUs on dead host");

        let mut tx = db.pool.begin().await.expect("Failed to begin tx");
        host_gpus::allocate_gpus(&mut tx, dead_host_id, vm_id, 1, None, None, None)
            .await
            .expect("Failed to allocate GPU on dead host");
        tx.commit().await.expect("Failed to commit");

        let gpu_request = GpuRequest {
            count: 1,
            vendor: Some("nvidia".to_string()),
            model: Some("A100".to_string()),
            min_vram_bytes: None,
        };
        let result = reallocate_vm_gpus(&db.app(), vm_id, target_host_id, &gpu_request).await;
        assert!(result.is_err());

        // The original allocation on the dead host must be untouched since
        // the transaction rolled back without committing the reallocation.
        let dead_host_gpus = host_gpus::list_by_host(&db.pool, dead_host_id)
            .await
            .expect("Failed to list GPUs on dead host");
        assert_eq!(dead_host_gpus[0].vm_id, Some(vm_id));
    }
}
