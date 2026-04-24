use std::path::PathBuf;
use std::time::Duration;

use tokio::process::Command;
use tracing::{debug, warn};

use super::{MappedDisk, StorageBackend};

/// Block (iSCSI) storage backend.
///
/// Pool config JSON: `{"portal": "host:port", "iqn": "iqn.2024-01.qarax:target0"}`.
/// Disk object config JSON: `{"lun": <u32>}`.
///
/// `attach` runs `iscsiadm` discovery + login so that the kernel creates
/// `/dev/disk/by-path/ip-<portal>-iscsi-<iqn>-lun-<lun>` for every LUN
/// exported by the target. `map` resolves that symlink for a given disk.
pub struct BlockBackend;

#[derive(serde::Deserialize, Debug)]
struct BlockPoolConfig {
    portal: String,
    iqn: String,
}

#[derive(serde::Deserialize, Debug)]
struct BlockDiskConfig {
    #[serde(default)]
    lun: u32,
}

fn validate_portal(portal: &str) -> anyhow::Result<()> {
    if portal.is_empty() {
        anyhow::bail!("portal is empty");
    }
    // Whitelist: alphanumeric plus the chars needed for host:port and IPv6 brackets.
    if portal
        .chars()
        .any(|c| !c.is_alphanumeric() && !matches!(c, '.' | ':' | '-' | '[' | ']'))
    {
        anyhow::bail!("portal contains illegal characters: {portal:?}");
    }
    Ok(())
}

fn validate_iqn(iqn: &str) -> anyhow::Result<()> {
    if !iqn.starts_with("iqn.") {
        anyhow::bail!("iqn must start with 'iqn.': {iqn:?}");
    }
    // Whitelist: IQN format is iqn.yyyy-mm.authority:unique-name
    if iqn
        .chars()
        .any(|c| !c.is_alphanumeric() && !matches!(c, '.' | ':' | '-'))
    {
        anyhow::bail!("iqn contains illegal characters: {iqn:?}");
    }
    Ok(())
}

async fn run_iscsiadm(args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("iscsiadm").args(args).output().await?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("iscsiadm {args:?} failed: {}", stderr.trim());
    }
}

fn by_path_symlink(portal: &str, iqn: &str, lun: u32) -> PathBuf {
    PathBuf::from(format!(
        "/dev/disk/by-path/ip-{portal}-iscsi-{iqn}-lun-{lun}"
    ))
}

#[tonic::async_trait]
impl StorageBackend for BlockBackend {
    async fn attach(&self, _pool_id: &str, config_json: &str) -> anyhow::Result<String> {
        let cfg: BlockPoolConfig = serde_json::from_str(config_json)
            .map_err(|e| anyhow::anyhow!("Invalid BLOCK pool config JSON: {e}"))?;

        validate_portal(&cfg.portal)?;
        validate_iqn(&cfg.iqn)?;

        // Discover targets on the portal. This is idempotent.
        if let Err(e) =
            run_iscsiadm(&["-m", "discovery", "-t", "sendtargets", "-p", &cfg.portal]).await
        {
            warn!(error = %e, portal = %cfg.portal, "iscsiadm discovery failed");
            return Err(e);
        }

        // Log in to the target. Already-logged-in returns non-zero on some
        // versions, so check for an existing session first.
        let sessions = run_iscsiadm(&["-m", "session"]).await.unwrap_or_default();
        let already_logged_in = sessions
            .lines()
            .any(|line| line.split_whitespace().any(|token| token == cfg.iqn));

        if !already_logged_in {
            run_iscsiadm(&["-m", "node", "-T", &cfg.iqn, "-p", &cfg.portal, "--login"]).await?;
        }

        Ok(format!(
            "iSCSI target {} @ {} attached",
            cfg.iqn, cfg.portal
        ))
    }

    async fn detach(&self, _pool_id: &str, config_json: &str) -> anyhow::Result<()> {
        let cfg: BlockPoolConfig = serde_json::from_str(config_json)
            .map_err(|e| anyhow::anyhow!("Invalid BLOCK pool config JSON: {e}"))?;

        validate_portal(&cfg.portal)?;
        validate_iqn(&cfg.iqn)?;

        let sessions = run_iscsiadm(&["-m", "session"]).await.unwrap_or_default();
        let logged_in = sessions
            .lines()
            .any(|line| line.split_whitespace().any(|token| token == cfg.iqn));

        if logged_in {
            run_iscsiadm(&["-m", "node", "-T", &cfg.iqn, "-p", &cfg.portal, "--logout"]).await?;
        }

        Ok(())
    }

    async fn map(&self, _vm_id: &str, config: &serde_json::Value) -> anyhow::Result<MappedDisk> {
        let portal = config
            .get("portal")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("block disk config missing 'portal'"))?;
        let iqn = config
            .get("iqn")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("block disk config missing 'iqn'"))?;
        let disk: BlockDiskConfig = serde_json::from_value(config.clone())
            .map_err(|e| anyhow::anyhow!("Invalid BLOCK disk config: {e}"))?;

        validate_portal(portal)?;
        validate_iqn(iqn)?;

        let symlink = by_path_symlink(portal, iqn, disk.lun);

        // The udev symlink may appear a moment after login; retry briefly.
        for _ in 0..20 {
            if tokio::fs::metadata(&symlink).await.is_ok() {
                let resolved = tokio::fs::canonicalize(&symlink).await?;
                debug!(
                    "BLOCK map resolved {} -> {}",
                    symlink.display(),
                    resolved.display()
                );
                return Ok(MappedDisk {
                    device_path: resolved.to_string_lossy().into_owned(),
                });
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        anyhow::bail!("iSCSI device {} did not appear in time", symlink.display());
    }

    async fn unmap(&self, _vm_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn recover(&self) -> anyhow::Result<()> {
        // Kernel restores sessions from node.startup=automatic records on boot.
        Ok(())
    }
}
