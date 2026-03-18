//! Cloud Hypervisor integration using the cloud-hypervisor-sdk
//!
//! This module provides VM management using the Cloud Hypervisor SDK,
//! with custom process management (spawning CH directly instead of via tmux).

pub(crate) mod cloud_init;
mod manager;

pub use cloud_hypervisor_sdk::models;
pub use manager::{VmManager, VmManagerError};
