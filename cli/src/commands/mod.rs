pub mod audit_log;
pub mod boot_source;
pub mod configure;
pub mod hook;
pub mod host;
pub mod instance_type;
pub mod job;
pub mod network;
pub mod sandbox;
pub mod security_group;
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
    let vms = api::vms::list(client, Some(name_or_id), &[]).await?;
    vms.into_iter()
        .next()
        .map(|vm| vm.id)
        .ok_or_else(|| anyhow::anyhow!("no VM named {:?}", name_or_id))
}

/// Resolve a host name or UUID string to a UUID.
pub async fn resolve_host_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let hosts = api::hosts::list(client, Some(name_or_id), None).await?;
    hosts
        .into_iter()
        .next()
        .map(|h| h.id)
        .ok_or_else(|| anyhow::anyhow!("no host named {:?}", name_or_id))
}

/// Resolve a storage pool name or UUID string to a UUID.
pub async fn resolve_pool_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let pools = api::storage::list_pools(client, Some(name_or_id)).await?;
    pools
        .into_iter()
        .next()
        .map(|p| p.id)
        .ok_or_else(|| anyhow::anyhow!("no storage pool named {:?}", name_or_id))
}

/// Resolve a storage object name or UUID string to a UUID.
pub async fn resolve_object_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let objects = api::storage::list_objects(client, Some(name_or_id), None, None).await?;
    objects
        .into_iter()
        .next()
        .map(|o| o.id)
        .ok_or_else(|| anyhow::anyhow!("no storage object named {:?}", name_or_id))
}

/// Resolve a network name or UUID string to a UUID.
pub async fn resolve_network_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let networks = api::networks::list(client, Some(name_or_id)).await?;
    networks
        .into_iter()
        .next()
        .map(|n| n.id)
        .ok_or_else(|| anyhow::anyhow!("no network named {:?}", name_or_id))
}

/// Resolve a security-group name or UUID string to a UUID.
pub async fn resolve_security_group_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let groups = api::security_groups::list(client, Some(name_or_id)).await?;
    groups
        .into_iter()
        .next()
        .map(|group| group.id)
        .ok_or_else(|| anyhow::anyhow!("no security group named {:?}", name_or_id))
}

/// Resolve a lifecycle hook name or UUID string to a UUID.
pub async fn resolve_hook_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let hooks = api::hooks::list(client, Some(name_or_id)).await?;
    hooks
        .into_iter()
        .next()
        .map(|h| h.id)
        .ok_or_else(|| anyhow::anyhow!("no hook named {:?}", name_or_id))
}

/// Resolve a boot source name or UUID string to a UUID.
pub async fn resolve_boot_source_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let sources = api::boot_sources::list(client, Some(name_or_id)).await?;
    sources
        .into_iter()
        .next()
        .map(|bs| bs.id)
        .ok_or_else(|| anyhow::anyhow!("no boot source named {:?}", name_or_id))
}

/// Resolve a VM template name or UUID string to a UUID.
pub async fn resolve_vm_template_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let templates = api::vm_templates::list(client, Some(name_or_id)).await?;
    templates
        .into_iter()
        .next()
        .map(|template| template.id)
        .ok_or_else(|| anyhow::anyhow!("no VM template named {:?}", name_or_id))
}

/// Resolve an instance type name or UUID string to a UUID.
pub async fn resolve_instance_type_id(client: &Client, name_or_id: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let instance_types = api::instance_types::list(client, Some(name_or_id)).await?;
    instance_types
        .into_iter()
        .next()
        .map(|instance_type| instance_type.id)
        .ok_or_else(|| anyhow::anyhow!("no instance type named {:?}", name_or_id))
}

/// Resolve a snapshot name or UUID string to a UUID.
pub async fn resolve_snapshot_id(
    client: &Client,
    vm_id: Uuid,
    name_or_id: &str,
) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(name_or_id) {
        return Ok(id);
    }
    let snapshots = api::vms::list_snapshots(client, vm_id, Some(name_or_id)).await?;
    snapshots
        .into_iter()
        .next()
        .map(|s| s.id)
        .ok_or_else(|| anyhow::anyhow!("no snapshot named {:?}", name_or_id))
}

/// Parse a human-readable size string into bytes.
/// Accepts plain integers or suffixed values: GiB, GB, MiB, MB, KiB, KB.
/// Examples: "10GiB", "20GB", "512MiB", "1073741824"
pub fn parse_size(s: &str) -> anyhow::Result<i64> {
    const GIB: i64 = 1024 * 1024 * 1024;
    const GB: i64 = 1_000_000_000;
    const MIB: i64 = 1024 * 1024;
    const MB: i64 = 1_000_000;
    const KIB: i64 = 1024;
    const KB: i64 = 1000;

    let s = s.trim();
    let (num, mult): (&str, i64) = if let Some(n) = s.strip_suffix("GiB") {
        (n, GIB)
    } else if let Some(n) = s.strip_suffix("GB") {
        (n, GB)
    } else if let Some(n) = s.strip_suffix("MiB") {
        (n, MIB)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, MB)
    } else if let Some(n) = s.strip_suffix("KiB") {
        (n, KIB)
    } else if let Some(n) = s.strip_suffix("KB") {
        (n, KB)
    } else {
        (s, 1)
    };
    let value: i64 = num
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid size: {s}"))?;
    Ok(value * mult)
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

/// Build an accelerator_config JSON value from optional GPU CLI flags.
pub fn build_accelerator_config(
    gpu_count: Option<i32>,
    gpu_vendor: &Option<String>,
    gpu_model: &Option<String>,
    min_vram: Option<i64>,
) -> Option<serde_json::Value> {
    gpu_count.map(|count| {
        let mut config = serde_json::json!({ "gpu_count": count });
        if let Some(v) = gpu_vendor {
            config["gpu_vendor"] = serde_json::json!(v);
        }
        if let Some(m) = gpu_model {
            config["gpu_model"] = serde_json::json!(m);
        }
        if let Some(vram) = min_vram {
            config["min_vram_bytes"] = serde_json::json!(vram);
        }
        config
    })
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
