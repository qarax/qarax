//! Shared polling helpers for async CLI operations.

use anyhow::anyhow;
use tokio::time::{Duration, sleep};
use uuid::Uuid;

use crate::{api, client::Client};

/// Poll a transfer until it reaches `completed` or `failed`.
///
/// Prints a progress line to stderr that is overwritten on each tick.
pub async fn wait_for_transfer(
    client: &Client,
    pool_id: Uuid,
    transfer_id: Uuid,
) -> anyhow::Result<()> {
    use std::io::Write as _;
    loop {
        let t = api::transfers::get(client, pool_id, transfer_id).await?;
        match t.status.as_str() {
            "completed" => {
                eprintln!("\r[completed]                        ");
                return Ok(());
            }
            "failed" => {
                return Err(anyhow!(
                    "Transfer {} failed: {}",
                    transfer_id,
                    t.error_message
                        .unwrap_or_else(|| "unknown error".to_string())
                ));
            }
            status => {
                let pct = match (t.transferred_bytes, t.total_bytes) {
                    (xfer, Some(total)) if total > 0 => {
                        format!("{:.0}%", xfer as f64 / total as f64 * 100.0)
                    }
                    _ => "--%".to_string(),
                };
                eprint!("\r[{status}] {pct}   ");
                let _ = std::io::stderr().flush();
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

/// Poll a sandbox until it reaches `ready` or `error`.
pub async fn wait_for_sandbox(client: &Client, sandbox_id: Uuid) -> anyhow::Result<()> {
    use std::io::Write as _;
    loop {
        let s = api::sandboxes::get(client, sandbox_id).await?;
        match s.status.as_str() {
            "ready" => {
                eprintln!("\r[ready]                        ");
                return Ok(());
            }
            "error" => {
                return Err(anyhow!(
                    "Sandbox {} failed: {}",
                    sandbox_id,
                    s.error_message
                        .unwrap_or_else(|| "unknown error".to_string())
                ));
            }
            status => {
                eprint!("\r[{status}]   ");
                let _ = std::io::stderr().flush();
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

/// Poll a VM until it reaches the given target status string (e.g. `"shutdown"`).
///
/// Errors if the VM enters `"unknown"` status, which indicates a crash.
pub async fn wait_for_vm_status(client: &Client, vm_id: Uuid, target: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    loop {
        let vm = api::vms::get(client, vm_id).await?;
        if vm.status == target {
            eprintln!("\r[{target}]                        ");
            return Ok(());
        }
        if vm.status == "unknown" {
            return Err(anyhow!("VM {vm_id} entered 'unknown' status"));
        }
        eprint!("\r[{}]   ", vm.status);
        let _ = std::io::stderr().flush();
        sleep(Duration::from_secs(2)).await;
    }
}
