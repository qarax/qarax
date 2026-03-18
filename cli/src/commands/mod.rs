pub mod boot_source;
pub mod configure;
pub mod host;
pub mod instance_type;
pub mod job;
pub mod network;
pub mod storage;
pub mod transfer;
pub mod vm;
pub mod vm_template;

use uuid::Uuid;

#[derive(clap::ValueEnum, Clone, Copy, Default, Debug)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Yaml,
}

use crate::{api, client::Client};

/// Resolve a VM name or UUID string to a UUID.
pub async fn resolve_vm_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let vms = api::vms::list(client).await?;
    vms.into_iter()
        .find(|vm| vm.name == name_or_id)
        .map(|vm| vm.id)
        .ok_or_else(|| anyhow::anyhow!("no VM named {:?}", name_or_id))
}

/// Resolve a host name or UUID string to a UUID.
pub async fn resolve_host_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let hosts = api::hosts::list(client).await?;
    hosts
        .into_iter()
        .find(|h| h.name == name_or_id)
        .map(|h| h.id)
        .ok_or_else(|| anyhow::anyhow!("no host named {:?}", name_or_id))
}

/// Resolve a storage pool name or UUID string to a UUID.
pub async fn resolve_pool_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let pools = api::storage::list_pools(client).await?;
    pools
        .into_iter()
        .find(|p| p.name == name_or_id)
        .map(|p| p.id)
        .ok_or_else(|| anyhow::anyhow!("no storage pool named {:?}", name_or_id))
}

/// Resolve a storage object name or UUID string to a UUID.
pub async fn resolve_object_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let objects = api::storage::list_objects(client).await?;
    objects
        .into_iter()
        .find(|o| o.name == name_or_id)
        .map(|o| o.id)
        .ok_or_else(|| anyhow::anyhow!("no storage object named {:?}", name_or_id))
}

/// Resolve a network name or UUID string to a UUID.
pub async fn resolve_network_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let networks = api::networks::list(client).await?;
    networks
        .into_iter()
        .find(|n| n.name == name_or_id)
        .map(|n| n.id)
        .ok_or_else(|| anyhow::anyhow!("no network named {:?}", name_or_id))
}

/// Resolve a boot source name or UUID string to a UUID.
pub async fn resolve_boot_source_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let sources = api::boot_sources::list(client).await?;
    sources
        .into_iter()
        .find(|bs| bs.name == name_or_id)
        .map(|bs| bs.id)
        .ok_or_else(|| anyhow::anyhow!("no boot source named {:?}", name_or_id))
}

/// Resolve a VM template name or UUID string to a UUID.
pub async fn resolve_vm_template_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let templates = api::vm_templates::list(client).await?;
    templates
        .into_iter()
        .find(|template| template.name == name_or_id)
        .map(|template| template.id)
        .ok_or_else(|| anyhow::anyhow!("no VM template named {:?}", name_or_id))
}

/// Resolve an instance type name or UUID string to a UUID.
pub async fn resolve_instance_type_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let instance_types = api::instance_types::list(client).await?;
    instance_types
        .into_iter()
        .find(|instance_type| instance_type.name == name_or_id)
        .map(|instance_type| instance_type.id)
        .ok_or_else(|| anyhow::anyhow!("no instance type named {:?}", name_or_id))
}

/// Format a byte count as a human-readable string (GiB / MiB / KiB / B).
pub fn format_bytes(bytes: i64) -> String {
    const GIB: i64 = 1024 * 1024 * 1024;
    const MIB: i64 = 1024 * 1024;
    const KIB: i64 = 1024;

    if bytes >= GIB {
        format!("{:.1}G", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1}M", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1}K", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes}B")
    }
}

/// Print output in the requested format (JSON or YAML). Falls back to JSON for Table.
pub fn print_output<T: serde::Serialize>(value: &T, format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Yaml => println!("{}", serde_yaml::to_string(value)?),
        OutputFormat::Json | OutputFormat::Table => {
            println!("{}", serde_json::to_string_pretty(value)?)
        }
    }

    Ok(())
}
