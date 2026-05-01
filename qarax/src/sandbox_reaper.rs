/// Background task that periodically deletes sandboxes that have exceeded their idle timeout.
use tokio::time::{Duration, interval};
use tracing::{info, warn};

use crate::sandbox_runtime::destroy_vm;
use crate::{
    App,
    model::{sandboxes, sandboxes::SandboxStatus},
};

pub async fn start_sandbox_reaper(env: App) {
    let mut ticker = interval(Duration::from_secs(15));

    loop {
        ticker.tick().await;

        if env.maintenance_mode() {
            continue;
        }

        let expired = match sandboxes::list_expired(env.pool()).await {
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
                sandboxes::update_status(env.pool(), sandbox.id, SandboxStatus::Destroying, None)
                    .await
            {
                warn!(
                    sandbox_id = %sandbox.id,
                    error = %e,
                    "Sandbox reaper: failed to update status to DESTROYING"
                );
                continue;
            }

            destroy_vm(env.pool(), sandbox.vm_id).await;
        }
    }
}
