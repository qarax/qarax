use sqlx::PgPool;
use tokio::time::{Duration, Instant, interval, sleep_until};
use uuid::Uuid;

use crate::{
    App,
    errors::Error,
    grpc_client::NodeClient,
    handlers::vm::handler::start_vm_internal,
    model::{
        events, hosts, jobs,
        sandboxes::NewSandbox,
        vm_templates,
        vms::{self, Hypervisor, NewVm, ResolvedNewVm, VmStatus},
    },
};

pub(crate) async fn resolve_sandbox_vm(
    env: &App,
    req: &NewSandbox,
) -> Result<ResolvedNewVm, Error> {
    let sandbox_hypervisor = vm_templates::get(env.pool(), req.vm_template_id)
        .await
        .map_err(Error::Sqlx)?
        .hypervisor
        .unwrap_or(Hypervisor::Firecracker);

    let new_vm = NewVm {
        name: req.name.clone(),
        tags: None,
        vm_template_id: Some(req.vm_template_id),
        instance_type_id: req.instance_type_id,
        hypervisor: Some(sandbox_hypervisor),
        architecture: None,
        boot_vcpus: None,
        max_vcpus: None,
        cpu_topology: None,
        kvm_hyperv: None,
        memory_size: None,
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
        network_id: req.network_id,
        networks: None,
        security_group_ids: None,
        accelerator_config: None,
        numa_config: None,
        persistent_upper_pool_id: None,
        placement_policy: None,
        guest_agent: Some(true),
        ha_enabled: None,
        config: serde_json::json!({}),
    };

    let resolved_vm = vms::resolve_create_request(env.pool(), new_vm).await?;
    Ok(resolved_vm)
}

#[derive(Debug)]
pub(crate) enum ReadyOutcome {
    Ready,
    Failed(String),
    TimedOut,
    Gone,
}

/// What a new sandbox VM waits on before it can be ready.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ReadyWait {
    /// An OCI VM pulling its image; the watcher starts it once it reaches
    /// `Created`.
    PullingImage { job_id: Uuid },
    /// A VM whose start job is running.
    Starting { job_id: Uuid },
}

impl ReadyWait {
    /// The job whose failure fails the wait.
    pub(crate) fn job_id(self) -> Uuid {
        match self {
            ReadyWait::PullingImage { job_id } | ReadyWait::Starting { job_id } => job_id,
        }
    }
}

/// Event-driven wait until the sandbox VM either reaches Running or fails.
///
/// Listens on the broadcast event bus emitted by `vms::transition`, with a
/// low-cadence job-status poll as a backstop for failures that don't transition
/// the VM (e.g. `start_vm` failing reverts the VM to its previous state and only
/// marks the job as failed). For OCI sandboxes, transitioning to `Created`
/// triggers `start_vm_internal` exactly once.
pub(crate) async fn watch_for_ready(env: &App, vm_id: Uuid, mut wait: ReadyWait) -> ReadyOutcome {
    // Subscribe first so we don't miss events between the initial poll and the loop.
    let mut events_rx = events::subscribe();

    let cfg = env.sandbox();
    let deadline = Instant::now()
        + Duration::from_secs(
            cfg.ready_watcher_interval_secs * cfg.ready_watcher_max_attempts as u64,
        );
    // Backstop poll just for the current job, used to detect start-job failures
    // that don't change VM state. Cadence is intentionally coarse.
    let mut job_backstop = interval(Duration::from_secs(cfg.ready_watcher_interval_secs.max(5)));
    job_backstop.tick().await; // consume the immediate first tick

    // Initial state check (covers races where the VM is already Running/failed by
    // the time we subscribe, and gets OCI sandboxes off the launchpad).
    match vms::get(env.pool(), vm_id).await {
        Ok(vm) => {
            if let Some(outcome) = handle_vm_status(env, vm.status, &mut wait, vm_id).await {
                return outcome;
            }
        }
        Err(sqlx::Error::RowNotFound) => return ReadyOutcome::Gone,
        Err(e) => {
            tracing::warn!(vm_id = %vm_id, error = %e, "watch_for_ready: initial VM fetch failed");
        }
    }

    loop {
        tokio::select! {
            biased;

            _ = sleep_until(deadline) => return ReadyOutcome::TimedOut,

            evt = events_rx.recv() => {
                match evt {
                    Ok(evt) if evt.vm_id == vm_id => {
                        let parsed: Option<VmStatus> = evt.new_status.parse().ok();
                        if let Some(status) = parsed
                            && let Some(outcome) = handle_vm_status(env, status, &mut wait, vm_id).await
                        {
                            return outcome;
                        }
                    }
                    Ok(_) => { /* event for a different VM */ }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // We may have missed events; re-sync from the DB.
                        match vms::get(env.pool(), vm_id).await {
                            Ok(vm) => {
                                if let Some(outcome) = handle_vm_status(env, vm.status, &mut wait, vm_id)
                                .await
                                {
                                    return outcome;
                                }
                            }
                            Err(sqlx::Error::RowNotFound) => return ReadyOutcome::Gone,
                            Err(e) => {
                                tracing::warn!(vm_id = %vm_id, error = %e, "watch_for_ready: lagged re-sync failed");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // Should not happen — bus is a static; treat as timeout to be safe.
                        return ReadyOutcome::TimedOut;
                    }
                }
            }

            _ = job_backstop.tick() => {
                if let Ok(job) = jobs::get(env.pool(), wait.job_id()).await
                    && job.status == jobs::JobStatus::Failed
                {
                    let msg = job.error.unwrap_or_else(|| "VM failed to start".to_string());
                    return ReadyOutcome::Failed(msg);
                }
            }
        }
    }
}

/// React to a VM status. Returns Some(outcome) when the wait should terminate.
async fn handle_vm_status(
    env: &App,
    status: VmStatus,
    wait: &mut ReadyWait,
    vm_id: Uuid,
) -> Option<ReadyOutcome> {
    match status {
        VmStatus::Running => Some(ReadyOutcome::Ready),
        VmStatus::Shutdown | VmStatus::Unknown => {
            Some(ReadyOutcome::Failed("VM failed to start".to_string()))
        }
        VmStatus::Created => match *wait {
            ReadyWait::PullingImage { .. } => match start_vm_internal(env, vm_id).await {
                Ok(job_id) => {
                    *wait = ReadyWait::Starting { job_id };
                    tracing::info!(
                        vm_id = %vm_id,
                        job_id = %job_id,
                        "OCI sandbox image pulled; starting VM"
                    );
                    None
                }
                Err(e) => Some(ReadyOutcome::Failed(e.to_string())),
            },
            ReadyWait::Starting { .. } => None,
        },
        VmStatus::Pending | VmStatus::Paused | VmStatus::Migrating | VmStatus::Committing => None,
    }
}

pub(crate) async fn destroy_vm(pool: &PgPool, vm_id: Uuid) {
    use crate::model::host_gpus;

    if let Err(e) = host_gpus::deallocate_by_vm(pool, vm_id).await {
        tracing::warn!(vm_id = %vm_id, error = %e, "Failed to deallocate GPUs for sandbox VM");
    }

    let vm = match vms::get(pool, vm_id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(vm_id = %vm_id, error = %e, "Failed to get sandbox VM for deletion");
            let _ = vms::delete(pool, vm_id).await;
            return;
        }
    };

    if let Some(host_id) = vm.host_id
        && let Ok(Some(host)) = hosts::get_by_id(pool, host_id).await
    {
        let client = NodeClient::new(&host.address, host.port as u16);
        if let Err(e) = client.delete_vm(vm_id).await {
            let not_found = e
                .downcast_ref::<crate::errors::Error>()
                .map(|err| matches!(err, crate::errors::Error::NotFound))
                .unwrap_or(false);
            if !not_found {
                tracing::warn!(vm_id = %vm_id, error = %e, "delete_vm on node failed (ignoring)");
            }
        }
    }

    if let Err(e) = vms::delete(pool, vm_id).await {
        tracing::error!(vm_id = %vm_id, error = %e, "Failed to delete sandbox VM from DB");
    }
}
