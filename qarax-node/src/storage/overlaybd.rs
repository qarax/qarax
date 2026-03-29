use std::path::PathBuf;
use std::sync::Arc;

use tracing::info;

use super::{MappedDisk, StorageBackend};
use crate::overlaybd::OverlayBdManager;

pub struct OverlayBdBackend {
    manager: Arc<OverlayBdManager>,
    qarax_init_binary: Option<PathBuf>,
}

impl OverlayBdBackend {
    pub fn new(manager: Arc<OverlayBdManager>, qarax_init_binary: Option<PathBuf>) -> Self {
        Self {
            manager,
            qarax_init_binary,
        }
    }

    pub fn manager(&self) -> &Arc<OverlayBdManager> {
        &self.manager
    }
}

#[tonic::async_trait]
impl StorageBackend for OverlayBdBackend {
    async fn attach(&self, _pool_id: &str, config_json: &str) -> anyhow::Result<String> {
        let cfg: serde_json::Value = serde_json::from_str(config_json)
            .map_err(|e| anyhow::anyhow!("Invalid OverlayBD pool config JSON: {}", e))?;

        let url = cfg
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("OverlayBD pool config missing 'url' field"))?;

        // Probe the OCI registry v2 endpoint.
        let probe = format!("{}/v2/", url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;
        let response = client
            .get(&probe)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Cannot reach registry at {}: {}", probe, e))?;

        // A v2 registry returns 200 or 401 (auth required); both mean it is alive.
        let status = response.status();
        if status.is_success() || status.as_u16() == 401 {
            Ok(format!("OverlayBD registry {} reachable ({})", url, status))
        } else {
            Err(anyhow::anyhow!(
                "OverlayBD registry {} returned unexpected status {}",
                url,
                status
            ))
        }
    }

    async fn detach(&self, _pool_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn map(&self, vm_id: &str, config: &serde_json::Value) -> anyhow::Result<MappedDisk> {
        let image_ref = config
            .get("image_ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("OverlayBD disk config missing 'image_ref'"))?;
        let registry_url = config
            .get("registry_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("OverlayBD disk config missing 'registry_url'"))?;

        let upper_data_path = config.get("upper_data_path").and_then(|v| v.as_str());
        let upper_index_path = config.get("upper_index_path").and_then(|v| v.as_str());

        let mounted = self
            .manager
            .mount(
                vm_id,
                image_ref,
                registry_url,
                upper_data_path,
                upper_index_path,
            )
            .await?;
        let device_path = mounted.device_path.clone();

        // Inject qarax-init into the mounted block device so the VM
        // boots with our init binary as PID 1.
        if let Some(init_binary) = &self.qarax_init_binary {
            self.manager
                .inject_init(vm_id, &device_path, image_ref, registry_url, init_binary)
                .await?;
        }

        info!(
            "OverlayBD mapped for VM {}: {} (init injected: {})",
            vm_id,
            device_path,
            self.qarax_init_binary.is_some()
        );

        Ok(MappedDisk { device_path })
    }

    async fn unmap(&self, vm_id: &str) -> anyhow::Result<()> {
        self.manager.unmount(vm_id).await;
        Ok(())
    }

    async fn recover(&self) -> anyhow::Result<()> {
        self.manager.recover().await;
        Ok(())
    }
}
