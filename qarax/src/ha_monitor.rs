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

use sqlx::{Postgres, Transaction};

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
    vms::{self, Vm, VmOp, VmStatus},
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
        .filter(|vm| vm.ha_enabled && VmOp::Failover.allows(vm.status))
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

    // Best-effort: if the dead host is partially alive, remove the old copy
    // before starting elsewhere. Use a short timeout since the host is
    // already known to be unreachable.
    let old_client = NodeClient::new(&dead_host.address, dead_host.port as u16);
    let cleanup_timeout = Duration::from_secs(2);
    let _ = tokio::time::timeout(cleanup_timeout, old_client.force_stop_vm(vm.id)).await;
    let _ = tokio::time::timeout(cleanup_timeout, old_client.delete_vm(vm.id)).await;

    let previous_status =
        move_vm_to_failover_target(env, vm, dead_host.id, target.id, &gpu_request).await?;

    let start_job_id = match start_vm_internal(env, vm.id).await {
        Ok(job_id) => job_id,
        Err(e) => {
            // Put the VM back on the dead host so the HA monitor picks it up
            // as a candidate again on a later tick, instead of leaving it
            // stranded in `Created` on the target host.
            if let Err(revert_err) =
                revert_failover_move(env, vm, previous_status, dead_host.id, &gpu_request).await
            {
                warn!(
                    "HA monitor: failed to put VM {} back on host {} after its start failed: {}",
                    vm.id, dead_host.name, revert_err
                );
            }
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

/// Point the VM at `target_host_id` in one transaction: status `Created`
/// (which makes the start path re-create it there), the host, the
/// `failed_over_from` marker that gets the stale copy cleaned up if the dead
/// host recovers, and its GPUs. Returns the status the VM left.
async fn move_vm_to_failover_target(
    env: &App,
    vm: &Vm,
    dead_host_id: Uuid,
    target_host_id: Uuid,
    gpu_request: &Option<hosts::GpuRequest>,
) -> Result<VmStatus, Error> {
    let mut config = vm.config.clone();
    vms::set_failed_over_from(&mut config, Some(dead_host_id));

    let mut tx = env.pool().begin().await?;
    let previous = vms::transition_tx(
        &mut tx,
        vm.id,
        VmOp::Failover.allowed_from(),
        VmOp::Failover.target(),
    )
    .await?
    .ok_or_else(|| {
        Error::Conflict(format!(
            "VM {} is no longer running, paused or unknown",
            vm.id
        ))
    })?;
    vms::update_config(tx.as_mut(), vm.id, &config).await?;
    vms::update_host_id(tx.as_mut(), vm.id, target_host_id).await?;
    if let Some(gpu_request) = gpu_request {
        reallocate_vm_gpus(&mut tx, vm.id, target_host_id, gpu_request).await?;
    }
    tx.commit().await?;

    vms::announce_transition(env.pool(), vm.id, previous, VmOp::Failover.target()).await;
    Ok(previous)
}

/// Undo [`move_vm_to_failover_target`] in one transaction. A VM that is no
/// longer `Created` was taken over by someone else since, and stays as it is.
async fn revert_failover_move(
    env: &App,
    vm: &Vm,
    previous_status: VmStatus,
    dead_host_id: Uuid,
    gpu_request: &Option<hosts::GpuRequest>,
) -> Result<(), Error> {
    let mut tx = env.pool().begin().await?;
    if vms::transition_tx(&mut tx, vm.id, &[VmStatus::Created], previous_status)
        .await?
        .is_none()
    {
        return Ok(());
    }
    vms::update_config(tx.as_mut(), vm.id, &vm.config).await?;
    vms::update_host_id(tx.as_mut(), vm.id, dead_host_id).await?;
    if let Some(gpu_request) = gpu_request {
        reallocate_vm_gpus(&mut tx, vm.id, dead_host_id, gpu_request).await?;
    }
    tx.commit().await?;

    vms::announce_transition(env.pool(), vm.id, VmStatus::Created, previous_status).await;
    Ok(())
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
        vendor: allocated.iter().find_map(|g| g.vendor.clone()),
        model: allocated.iter().find_map(|g| g.model.clone()),
        min_vram_bytes: allocated.iter().filter_map(|g| g.vram_bytes).min(),
    }))
}

/// Move the VM's GPU allocation to `target_host_id` inside the caller's
/// transaction, which must roll back on error so a partial allocation never
/// leaves the VM without its required GPUs.
async fn reallocate_vm_gpus(
    tx: &mut Transaction<'_, Postgres>,
    vm_id: Uuid,
    target_host_id: Uuid,
    gpu_request: &hosts::GpuRequest,
) -> Result<(), Error> {
    host_gpus::deallocate_by_vm_tx(tx, vm_id).await?;
    let allocated = host_gpus::allocate_gpus(
        tx,
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
    use uuid::Uuid;

    use super::{collect_failover_candidates, gpu_request_for_vm, reallocate_vm_gpus};
    use crate::{
        model::{
            host_gpus::{self, GpuDiscovery},
            hosts::{GpuRequest, HostStatus},
            vms::{self, ResolvedNewVm, VmStatus},
        },
        test_support::{TestDatabase, resolved_vm},
    };

    /// Inserts a VM on `host_id` through the writer the create handler uses.
    async fn insert_vm(
        db: &TestDatabase,
        name: &str,
        host_id: Uuid,
        status: VmStatus,
        ha: bool,
    ) -> Uuid {
        let vm = ResolvedNewVm {
            config: serde_json::json!({ "ha_enabled": ha }),
            ..resolved_vm(name)
        };
        let mut tx = db.pool.begin().await.expect("Failed to begin tx");
        let vm_id = vms::create_tx_with_status(&mut tx, &vm, Some(host_id), status)
            .await
            .expect("Failed to insert VM");
        tx.commit().await.expect("Failed to commit");
        vm_id
    }

    #[tokio::test]
    async fn only_active_ha_vms_are_failover_candidates() {
        let db = TestDatabase::new().await;
        let host_id = db.add_host("ha-test-host", HostStatus::Down).await;

        let mut expected = Vec::new();
        for (name, status) in [
            ("ha-running", VmStatus::Running),
            ("ha-paused", VmStatus::Paused),
            ("ha-unknown", VmStatus::Unknown),
        ] {
            expected.push(insert_vm(&db, name, host_id, status, true).await);
        }
        insert_vm(&db, "plain-running", host_id, VmStatus::Running, false).await;
        insert_vm(&db, "ha-stopped", host_id, VmStatus::Shutdown, true).await;
        insert_vm(&db, "ha-never-started", host_id, VmStatus::Created, true).await;

        let candidates = collect_failover_candidates(&db.app(), host_id)
            .await
            .expect("Failed to collect candidates");

        let mut ids: Vec<Uuid> = candidates.iter().map(|vm| vm.id).collect();
        ids.sort();
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[tokio::test]
    async fn failed_over_vms_are_listed_by_source_host() {
        let db = TestDatabase::new().await;
        let host_id = db.add_host("ha-test-host", HostStatus::Down).await;
        let vm_id = insert_vm(&db, "ha-failed-over", host_id, VmStatus::Running, true).await;

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
        // The HA flag must survive the marker updates.
        assert!(listed[0].ha_enabled);

        vms::set_failed_over_from(&mut config, None);
        vms::update_config(&db.pool, vm_id, &config)
            .await
            .expect("Failed to clear marker");

        let listed = vms::list_failed_over_from(&db.pool, host_id)
            .await
            .expect("Failed to list failed-over VMs");
        assert!(listed.is_empty());
        assert!(vms::get(&db.pool, vm_id).await.unwrap().ha_enabled);
    }

    #[tokio::test]
    async fn gpu_request_reflects_current_allocation() {
        let db = TestDatabase::new().await;
        let host_id = db.add_host("ha-test-host", HostStatus::Down).await;
        let vm_id = insert_vm(&db, "gpu-vm", host_id, VmStatus::Running, true).await;

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
        let host_id = db.add_host("ha-test-host", HostStatus::Down).await;
        let vm_id = insert_vm(&db, "no-gpu-vm", host_id, VmStatus::Running, true).await;

        let vm = vms::get(&db.pool, vm_id).await.expect("Failed to get VM");
        let gpu_request = gpu_request_for_vm(&db.app(), &vm)
            .await
            .expect("Failed to compute GPU request");
        assert!(gpu_request.is_none());
    }

    #[tokio::test]
    async fn reallocate_vm_gpus_moves_allocation_to_target_host() {
        let db = TestDatabase::new().await;
        let dead_host_id = db.add_host("ha-test-host-dead", HostStatus::Down).await;
        let target_host_id = db.add_host("ha-test-host-target", HostStatus::Down).await;
        let vm_id = insert_vm(&db, "gpu-vm", dead_host_id, VmStatus::Running, true).await;

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
        let mut tx = db.pool.begin().await.expect("Failed to begin tx");
        reallocate_vm_gpus(&mut tx, vm_id, target_host_id, &gpu_request)
            .await
            .expect("Failed to reallocate GPUs");
        tx.commit().await.expect("Failed to commit");

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
        let dead_host_id = db.add_host("ha-test-host-dead", HostStatus::Down).await;
        let target_host_id = db.add_host("ha-test-host-target", HostStatus::Down).await;
        let vm_id = insert_vm(&db, "gpu-vm", dead_host_id, VmStatus::Running, true).await;

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
        let mut tx = db.pool.begin().await.expect("Failed to begin tx");
        let result = reallocate_vm_gpus(&mut tx, vm_id, target_host_id, &gpu_request).await;
        assert!(result.is_err());
        drop(tx);

        // The original allocation on the dead host must be untouched since
        // the transaction rolled back without committing the reallocation.
        let dead_host_gpus = host_gpus::list_by_host(&db.pool, dead_host_id)
            .await
            .expect("Failed to list GPUs on dead host");
        assert_eq!(dead_host_gpus[0].vm_id, Some(vm_id));
    }
}
