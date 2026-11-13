//! Shared polling helpers for async CLI operations.

use std::io::{IsTerminal as _, Write as _};
use std::time::Instant;

use anyhow::anyhow;
use crossterm::{
    cursor,
    terminal::{self, ClearType},
};
use tokio::time::{Duration, sleep};
use uuid::Uuid;

use crate::{
    api::{self, models::Transfer},
    client::Client,
};

/// Poll interval that starts short, so quick operations return promptly, and
/// backs off to 2 s for long ones.
struct Backoff(Duration);

impl Backoff {
    fn new() -> Self {
        Self(Duration::from_millis(250))
    }

    async fn tick(&mut self) {
        sleep(self.0).await;
        self.0 = (self.0 * 2).min(Duration::from_secs(2));
    }
}

/// A single stderr line overwritten on each poll. Draws nothing when stderr is
/// not a terminal, so scripts and logs don't collect carriage-return noise.
struct StatusLine {
    tty: bool,
}

impl StatusLine {
    fn new() -> Self {
        Self {
            tty: std::io::stderr().is_terminal(),
        }
    }

    fn update(&self, text: &str) {
        if self.tty {
            self.clear();
            let mut stderr = std::io::stderr();
            let _ = write!(stderr, "{text}");
            let _ = stderr.flush();
        }
    }

    fn clear(&self) {
        if self.tty {
            let _ = crossterm::execute!(
                std::io::stderr(),
                cursor::MoveToColumn(0),
                terminal::Clear(ClearType::CurrentLine),
            );
        }
    }
}

fn progress_bar(pct: usize) -> String {
    const WIDTH: usize = 30;
    let filled = WIDTH * pct / 100;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(WIDTH - filled))
}

/// Poll a job until it completes or fails, drawing its progress on stderr.
pub async fn wait_for_job(client: &Client, job_id: Uuid) -> anyhow::Result<()> {
    let started = Instant::now();
    let line = StatusLine::new();
    let mut backoff = Backoff::new();
    loop {
        let job = api::jobs::get(client, job_id).await?;
        match job.status.as_str() {
            "completed" => {
                line.clear();
                eprintln!("  done in {}s", started.elapsed().as_secs());
                return Ok(());
            }
            "failed" => {
                line.clear();
                return Err(anyhow!(
                    "Job {job_id} failed: {}",
                    job.error.unwrap_or_else(|| "unknown error".to_string())
                ));
            }
            status => {
                let pct = job.progress.unwrap_or(0).clamp(0, 99) as usize;
                let desc = job.description.as_deref().unwrap_or(status);
                line.update(&format!(
                    "  {} {pct:>3}%  {desc}  {}s",
                    progress_bar(pct),
                    started.elapsed().as_secs()
                ));
                backoff.tick().await;
            }
        }
    }
}

/// Poll a transfer until it reaches `completed` or `failed`.
pub async fn wait_for_transfer(
    client: &Client,
    pool_id: Uuid,
    transfer_id: Uuid,
) -> anyhow::Result<Transfer> {
    let line = StatusLine::new();
    let mut backoff = Backoff::new();
    loop {
        let t = api::transfers::get(client, pool_id, transfer_id).await?;
        match t.status.as_str() {
            "completed" => {
                line.clear();
                return Ok(t);
            }
            "failed" => {
                line.clear();
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
                line.update(&format!("[{status}] {pct}"));
                backoff.tick().await;
            }
        }
    }
}

/// Poll a sandbox until it reaches `ready` or `error`.
pub async fn wait_for_sandbox(client: &Client, sandbox_id: Uuid) -> anyhow::Result<()> {
    let line = StatusLine::new();
    let mut backoff = Backoff::new();
    loop {
        let s = api::sandboxes::get(client, sandbox_id).await?;
        match s.status.as_str() {
            "ready" => {
                line.clear();
                return Ok(());
            }
            "error" => {
                line.clear();
                return Err(anyhow!(
                    "Sandbox {} failed: {}",
                    sandbox_id,
                    s.error_message
                        .unwrap_or_else(|| "unknown error".to_string())
                ));
            }
            status => {
                line.update(&format!("[{status}]"));
                backoff.tick().await;
            }
        }
    }
}

/// Poll a VM until it reaches the given target status string (e.g. `"shutdown"`).
///
/// Errors if the VM enters `"unknown"` status, which indicates a crash.
pub async fn wait_for_vm_status(client: &Client, vm_id: Uuid, target: &str) -> anyhow::Result<()> {
    let line = StatusLine::new();
    let mut backoff = Backoff::new();
    loop {
        let vm = api::vms::get(client, vm_id).await?;
        if vm.status == target {
            line.clear();
            return Ok(());
        }
        if vm.status == "unknown" {
            line.clear();
            return Err(anyhow!("VM {vm_id} entered 'unknown' status"));
        }
        line.update(&format!("[{}]", vm.status));
        backoff.tick().await;
    }
}
