/// Background task that periodically deletes sandboxes that have exceeded their idle timeout.
use std::sync::Arc;

use sqlx::PgPool;
use tokio::time::{Duration, interval};
use tracing::{info, warn};

use crate::model::{sandboxes, sandboxes::SandboxStatus};

pub async fn start_sandbox_reaper(pool: Arc<PgPool>) {
    let mut ticker = interval(Duration::from_secs(15));

    loop {
        ticker.tick().await;

        let expired = match sandboxes::list_expired(&pool).await {
            Ok(list) => list,
            Err(e) => {
                warn!("Sandbox reaper: failed to query expired sandboxes: {}", e);
                continue;
            }
        };

        for sandbox in expired {
            info!(
                sandbox_id = %sandbox.id,
                vm_id = %sandbox.vm_id,
                "Reaping idle sandbox"
            );

            if let Err(e) =
                sandboxes::update_status(&pool, sandbox.id, SandboxStatus::Destroying, None).await
            {
                warn!(
                    sandbox_id = %sandbox.id,
                    error = %e,
                    "Sandbox reaper: failed to update status to DESTROYING"
                );
                continue;
            }

            destroy_vm(&pool, sandbox.vm_id).await;
        }
    }
}

async fn destroy_vm(pool: &PgPool, vm_id: uuid::Uuid) {
    use crate::grpc_client::NodeClient;
    use crate::model::{host_gpus, hosts, vms, vms::VmStatus};

    if let Err(e) = host_gpus::deallocate_by_vm(pool, vm_id).await {
        warn!(vm_id = %vm_id, error = %e, "Sandbox reaper: failed to deallocate GPUs");
    }

    let vm = match vms::get(pool, vm_id).await {
        Ok(v) => v,
        Err(e) => {
            warn!(vm_id = %vm_id, error = %e, "Sandbox reaper: VM not found, deleting DB row");
            let _ = vms::delete(pool, vm_id).await;
            return;
        }
    };

    if vm.status != VmStatus::Created
        && vm.status != VmStatus::Pending
        && let Some(host_id) = vm.host_id
        && let Ok(Some(host)) = hosts::get_by_id(pool, host_id).await
    {
        let client = NodeClient::new(&host.address, host.port as u16);
        if let Err(e) = client.delete_vm(vm_id).await {
            warn!(vm_id = %vm_id, error = %e, "Sandbox reaper: delete_vm on node failed (ignoring)");
        }
    }

    if let Err(e) = vms::delete(pool, vm_id).await {
        warn!(vm_id = %vm_id, error = %e, "Sandbox reaper: failed to delete VM from DB");
    }
}
