/// Background task that polls for pending webhook executions and delivers them.
/// Follows the same pattern as `vm_monitor.rs`.
use chrono::{Duration, Utc};
use sqlx::PgPool;
use tokio::time::{self, interval};
use tracing::{info, warn};

use crate::{
    App,
    model::lifecycle_hooks::{self, HookExecution},
};

/// Backoff durations for retries: 5s, 30s, 2m, 10m, 30m
const BACKOFF_SECS: [i64; 5] = [5, 30, 120, 600, 1800];

pub async fn start_hook_executor(env: App) {
    let client = reqwest::Client::builder()
        .timeout(time::Duration::from_secs(10))
        .build()
        .expect("failed to build reqwest client for hook executor");

    let mut ticker = interval(time::Duration::from_secs(2));

    loop {
        ticker.tick().await;

        if env.maintenance_mode() {
            continue;
        }

        #[cfg(feature = "otel")]
        let _cycle_start = std::time::Instant::now();

        let pending = match lifecycle_hooks::fetch_pending_executions(env.pool(), 20).await {
            Ok(execs) => execs,
            Err(e) => {
                warn!("hook executor: failed to fetch pending executions: {}", e);
                continue;
            }
        };

        for execution in pending {
            let pool = env.pool_arc();
            let client = client.clone();
            tokio::spawn(async move {
                deliver_hook(&pool, &client, execution).await;
            });
        }

        #[cfg(feature = "otel")]
        crate::vm_monitor::record_monitor_cycle("hook", _cycle_start);
    }
}

async fn deliver_hook(pool: &PgPool, client: &reqwest::Client, execution: HookExecution) {
    // Look up the hook to get URL and secret
    let hook = match lifecycle_hooks::get(pool, execution.hook_id).await {
        Ok(h) => h,
        Err(e) => {
            warn!(
                "hook executor: hook {} not found, marking execution {} as failed: {}",
                execution.hook_id, execution.id, e
            );
            let _ =
                lifecycle_hooks::mark_failed(pool, execution.id, &format!("hook not found: {}", e))
                    .await;
            return;
        }
    };

    let body = serde_json::to_string(&execution.payload).unwrap_or_default();

    let mut request = client
        .post(&hook.url)
        .header("Content-Type", "application/json")
        .header("X-Qarax-Event", "vm.status_changed");

    // HMAC-SHA256 signing if secret is set
    if let Some(ref secret) = hook.secret {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        type HmacSha256 = Hmac<Sha256>;

        if let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) {
            mac.update(body.as_bytes());
            let signature = hex::encode(mac.finalize().into_bytes());
            request = request.header("X-Qarax-Signature", format!("sha256={}", signature));
        }
    }

    match request.body(body).send().await {
        Ok(response) => {
            let status_code = response.status().as_u16() as i32;
            if response.status().is_success() {
                let resp_body = response.text().await.ok();
                info!(
                    "hook executor: delivered execution {} (HTTP {})",
                    execution.id, status_code
                );
                let _ = lifecycle_hooks::mark_delivered(
                    pool,
                    execution.id,
                    status_code,
                    resp_body.as_deref(),
                )
                .await;
            } else {
                let resp_body = response.text().await.unwrap_or_default();
                let error = format!("HTTP {}: {}", status_code, resp_body);
                handle_failure(pool, &execution, &error).await;
            }
        }
        Err(e) => {
            let error = format!("request error: {}", e);
            handle_failure(pool, &execution, &error).await;
        }
    }
}

async fn handle_failure(pool: &PgPool, execution: &HookExecution, error: &str) {
    let next_attempt = execution.attempt_count + 1;

    if next_attempt >= execution.max_attempts {
        warn!(
            "hook executor: execution {} exhausted all {} attempts, marking failed: {}",
            execution.id, execution.max_attempts, error
        );
        let _ = lifecycle_hooks::mark_failed(pool, execution.id, error).await;
    } else {
        let backoff_idx = (next_attempt as usize).min(BACKOFF_SECS.len() - 1);
        let backoff = Duration::seconds(BACKOFF_SECS[backoff_idx]);
        let next_retry = Utc::now() + backoff;

        warn!(
            "hook executor: execution {} attempt {} failed, retrying at {}: {}",
            execution.id, next_attempt, next_retry, error
        );
        let _ = lifecycle_hooks::mark_retry(pool, execution.id, error, next_retry).await;
    }
}
