/// Background task that periodically deletes sandboxes that have exceeded their idle timeout.
use std::sync::Arc;

use sqlx::PgPool;
use tokio::time::{Duration, interval};
use tracing::{info, warn};

use crate::model::{sandboxes, sandboxes::SandboxStatus};
use crate::sandbox_runtime::destroy_vm;

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
