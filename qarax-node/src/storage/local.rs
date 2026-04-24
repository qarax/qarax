use super::{MappedDisk, StorageBackend};

pub struct LocalBackend;

#[tonic::async_trait]
impl StorageBackend for LocalBackend {
    async fn attach(&self, pool_id: &str, config_json: &str) -> anyhow::Result<String> {
        let cfg: serde_json::Value =
            serde_json::from_str(config_json).unwrap_or_else(|_| serde_json::json!({}));

        let path_str = cfg.get("path").and_then(|v| v.as_str()).unwrap_or_default();

        let dir = if path_str.is_empty() {
            std::path::PathBuf::from(format!("/var/lib/qarax/pools/{}", pool_id))
        } else {
            std::path::PathBuf::from(path_str)
        };

        tokio::fs::create_dir_all(&dir).await?;
        Ok(format!("local dir {} ready", dir.display()))
    }

    async fn detach(&self, _pool_id: &str, _config_json: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn map(&self, _vm_id: &str, config: &serde_json::Value) -> anyhow::Result<MappedDisk> {
        let path = config
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("local disk config missing 'path'"))?;

        Ok(MappedDisk {
            device_path: path.to_string(),
        })
    }

    async fn unmap(&self, _vm_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
