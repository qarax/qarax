//! The HTTP integration tests, as one test binary: a module per resource, so
//! the control plane is linked into one executable and not one per file.

mod support;

mod audit_logs;
mod auth;
mod hosts;
mod lifecycle_hooks;
mod pagination;
mod sandbox_fork;
mod sandbox_pools;
mod snapshots;
mod storage_cleanup;
mod storage_pools;
mod vm_templates;
mod vms;
mod web_ui;
