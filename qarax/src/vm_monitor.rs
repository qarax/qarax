/// Background task that periodically reconciles VM status between the database
/// and the live state reported by qarax-node. Detects drift caused by node
/// restarts or unexpected VM terminations and updates the DB accordingly.
use std::collections::HashMap;
use tokio::time::{Duration, interval};
use tracing::{info, warn};
use uuid::Uuid;

use crate::App;
use crate::grpc_client::NodeClient;
use crate::model::{
    hosts,
    vms::{self, VmStatus},
};

pub async fn start_vm_monitor(env: App) {
    let mut ticker = interval(Duration::from_secs(30));

    loop {
        ticker.tick().await;

        if env.maintenance_mode() {
            continue;
        }

        #[cfg(feature = "otel")]
        let _cycle_start = std::time::Instant::now();

        let active_vms = match vms::list_active(env.pool()).await {
            Ok(vms) => vms,
            Err(e) => {
                warn!("VM monitor: failed to list active VMs: {}", e);
                continue;
            }
        };

        if active_vms.is_empty() {
            #[cfg(feature = "otel")]
            record_monitor_cycle("vm", _cycle_start);
            continue;
        }

        // Group VMs by host so we open one gRPC connection per host per tick
        let mut by_host: HashMap<Uuid, Vec<_>> = HashMap::new();
        for vm in active_vms {
            if let Some(host_id) = vm.host_id {
                by_host.entry(host_id).or_default().push(vm);
            }
        }

        let mut join_set = tokio::task::JoinSet::new();

        for (host_id, vms) in by_host {
            let env = env.clone();
            join_set.spawn(async move {
                let host = match hosts::get_by_id(env.pool(), host_id).await {
                    Ok(Some(h)) => h,
                    Ok(None) => {
                        warn!("VM monitor: host {} not found in DB", host_id);
                        return;
                    }
                    Err(e) => {
                        warn!("VM monitor: failed to look up host {}: {}", host_id, e);
                        return;
                    }
                };

                // One channel per host per tick; reused for all VMs on this host.
                let client = NodeClient::new(&host.address, host.port as u16);

                for vm in vms {
                    match client.get_vm_info(vm.id).await {
                        Ok(state) => {
                            let previous_status = vm.status;
                            let live_status = proto_status_to_db(state.status, previous_status);
                            if live_status != previous_status {
                                info!(
                                    "VM monitor: VM {} status changed from {:?} to {:?}",
                                    vm.id, previous_status, live_status
                                );
                                if let Err(e) =
                                    vms::update_status(env.pool(), vm.id, live_status).await
                                {
                                    warn!(
                                        "VM monitor: failed to update VM {} status: {}",
                                        vm.id, e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            if e.downcast_ref::<crate::errors::Error>()
                                .map(|e| matches!(e, crate::errors::Error::NotFound))
                                .unwrap_or(false)
                            {
                                if let Some(status) = status_when_missing_on_node(vm.status) {
                                    info!(
                                        "VM monitor: VM {} not found on node, marking as {:?}",
                                        vm.id, status
                                    );
                                    if let Err(db_err) =
                                        vms::update_status(env.pool(), vm.id, status).await
                                    {
                                        warn!(
                                            "VM monitor: failed to update VM {} status: {}",
                                            vm.id, db_err
                                        );
                                    }
                                }
                            } else {
                                warn!(
                                    "VM monitor: failed to get VM {} info from host {} (node may be down): {}",
                                    vm.id, host.name, e
                                );
                            }
                        }
                    }
                }
            });
        }

        while let Some(result) = join_set.join_next().await {
            if let Err(e) = result {
                warn!("VM monitor: host task panicked: {}", e);
            }
        }

        #[cfg(feature = "otel")]
        record_monitor_cycle("vm", _cycle_start);
    }
}

#[cfg(feature = "otel")]
pub fn record_monitor_cycle(monitor: &str, start: std::time::Instant) {
    use opentelemetry::KeyValue;

    let meter = opentelemetry::global::meter("qarax");
    let duration = start.elapsed().as_secs_f64();
    meter
        .f64_histogram("qarax.monitor.cycle.duration")
        .with_unit("s")
        .build()
        .record(duration, &[KeyValue::new("monitor", monitor.to_string())]);
    meter
        .u64_counter("qarax.monitor.cycles.total")
        .build()
        .add(1, &[KeyValue::new("monitor", monitor.to_string())]);
}

/// New status for a VM its host reports as unknown, or `None` to leave it.
///
/// A `Created` VM has a host (OCI image VMs are placed when their image is
/// imported) but is only defined on the node when it is first started, so
/// "not found" is its normal state. Marking it `Unknown` would also make
/// start skip the create request and fail with NotFound.
fn status_when_missing_on_node(current: VmStatus) -> Option<VmStatus> {
    match current {
        VmStatus::Created => None,
        _ => Some(VmStatus::Unknown),
    }
}

fn proto_status_to_db(status: i32, previous_status: VmStatus) -> VmStatus {
    // Proto VmStatus values:
    // VM_STATUS_UNKNOWN = 0, VM_STATUS_CREATED = 1, VM_STATUS_RUNNING = 2,
    // VM_STATUS_PAUSED = 3, VM_STATUS_SHUTDOWN = 4
    match status {
        // Cloud Hypervisor can report a stopped-but-still-defined VM as "Created".
        // Preserve the user-visible stopped state when we already knew this VM had
        // progressed past initial creation.
        1 if matches!(
            previous_status,
            VmStatus::Running | VmStatus::Paused | VmStatus::Shutdown
        ) =>
        {
            VmStatus::Shutdown
        }
        1 => VmStatus::Created,
        2 => VmStatus::Running,
        3 => VmStatus::Paused,
        4 => VmStatus::Shutdown,
        _ => VmStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{proto_status_to_db, status_when_missing_on_node};
    use crate::model::vms::VmStatus;

    #[test]
    fn never_started_vm_missing_on_node_stays_created() {
        assert_eq!(status_when_missing_on_node(VmStatus::Created), None);
    }

    #[test]
    fn deployed_vm_missing_on_node_becomes_unknown() {
        for status in [VmStatus::Running, VmStatus::Paused, VmStatus::Migrating] {
            assert_eq!(status_when_missing_on_node(status), Some(VmStatus::Unknown));
        }
    }

    #[test]
    fn created_state_stays_created_for_never_started_vms() {
        assert_eq!(proto_status_to_db(1, VmStatus::Created), VmStatus::Created);
    }

    #[test]
    fn created_state_normalizes_to_shutdown_after_stop() {
        assert_eq!(proto_status_to_db(1, VmStatus::Running), VmStatus::Shutdown);
        assert_eq!(proto_status_to_db(1, VmStatus::Paused), VmStatus::Shutdown);
        assert_eq!(
            proto_status_to_db(1, VmStatus::Shutdown),
            VmStatus::Shutdown
        );
    }
}
