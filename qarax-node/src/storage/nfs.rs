use super::{MappedDisk, StorageBackend};

pub struct NfsBackend;

/// Validate an NFS URL of the form `host:/path`.
///
/// Rejects blank values, missing colon-slash separator, and shell-injectable
/// characters that have no place in a hostname or export path.
fn validate_nfs_url(url: &str) -> anyhow::Result<()> {
    let (host, path) = url
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("Invalid NFS URL {url:?}: expected 'host:/path'"))?;

    if host.is_empty() {
        anyhow::bail!("Invalid NFS URL {url:?}: host is empty");
    }
    if !path.starts_with('/') {
        anyhow::bail!("Invalid NFS URL {url:?}: path must start with '/'");
    }

    let bad = |c: char| matches!(c, '\0' | '\n' | '\r' | ';' | '&' | '|' | '`' | '$' | '\\');
    if host.chars().any(bad) || path.chars().any(bad) {
        anyhow::bail!("Invalid NFS URL {url:?}: contains illegal characters");
    }

    Ok(())
}

#[tonic::async_trait]
impl StorageBackend for NfsBackend {
    async fn attach(&self, pool_id: &str, config_json: &str) -> anyhow::Result<String> {
        let cfg: serde_json::Value = serde_json::from_str(config_json)
            .map_err(|e| anyhow::anyhow!("Invalid NFS pool config JSON: {}", e))?;

        let url = cfg
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("NFS pool config missing 'url' field"))?;

        validate_nfs_url(url)?;

        let mount_point = format!("/var/lib/qarax/pools/{}", pool_id);
        tokio::fs::create_dir_all(&mount_point).await?;

        let output = tokio::process::Command::new("mount")
            .args(["-t", "nfs", "-o", "nolock", url, &mount_point])
            .output()
            .await?;

        if output.status.success() {
            Ok(format!("NFS {} mounted at {}", url, mount_point))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("mount failed: {}", stderr.trim()))
        }
    }

    async fn detach(&self, pool_id: &str) -> anyhow::Result<()> {
        let mount_point = format!("/var/lib/qarax/pools/{}", pool_id);

        let output = tokio::process::Command::new("umount")
            .arg(&mount_point)
            .output()
            .await?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("umount failed: {}", stderr.trim()))
        }
    }

    async fn map(&self, _vm_id: &str, config: &serde_json::Value) -> anyhow::Result<MappedDisk> {
        let path = config
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("NFS disk config missing 'path'"))?;

        Ok(MappedDisk {
            device_path: path.to_string(),
        })
    }

    async fn unmap(&self, _vm_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
