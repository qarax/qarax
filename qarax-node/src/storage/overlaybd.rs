use std::path::PathBuf;
use std::sync::Arc;

use tracing::info;

use super::{MappedDisk, StorageBackend};
use crate::overlaybd::OverlayBdManager;
use crate::rpc::node::DiskConfig;

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
        // OverlayBD disks are TCMU devices served by overlaybd-tcmu. Without
        // it every mount fails with ENOENT on the backstore's enable file, so
        // refuse the pool and keep the scheduler from sending image VMs here.
        // (The e2e stack runs it on one node only: two daemons can't share a
        // kernel's TCMU handler.)
        if !tcmu_daemon_running().await {
            anyhow::bail!(
                "overlaybd-tcmu is not running on this node, so it can't serve OverlayBD disks"
            );
        }

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

    async fn detach(&self, _pool_id: &str, _config_json: &str) -> anyhow::Result<()> {
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

    async fn has_mapping(&self, vm_id: &str) -> bool {
        self.manager.has_mapping(vm_id).await
    }

    async fn recover(&self) -> anyhow::Result<()> {
        self.manager.recover().await;
        Ok(())
    }
}

/// Present each disk of `disks` that names an OCI image as an OverlayBD
/// block device of `vm_id`: its image fields give way to the device's path.
/// Returns whether a disk was mapped, so that a caller that then fails to
/// start the VM can unmap it. Both hypervisors' backends map OCI disks so.
pub async fn map_oci_disks(
    backend: Option<&Arc<dyn StorageBackend>>,
    vm_id: &str,
    disks: &mut [DiskConfig],
) -> anyhow::Result<bool> {
    let mut mapped = false;
    for disk in disks.iter_mut() {
        let (Some(image_ref), Some(registry_url)) = (&disk.oci_image_ref, &disk.registry_url)
        else {
            continue;
        };
        let backend = backend.ok_or_else(|| {
            anyhow::anyhow!(
                "Disk {} requests OverlayBD but no OverlayBD backend is configured",
                disk.id
            )
        })?;

        let mut disk_config = serde_json::json!({
            "image_ref": image_ref,
            "registry_url": registry_url,
        });
        if let Some(upper_data) = &disk.upper_data_path {
            disk_config["upper_data_path"] = upper_data.clone().into();
        }
        if let Some(upper_index) = &disk.upper_index_path {
            disk_config["upper_index_path"] = upper_index.clone().into();
        }
        let device = backend.map(vm_id, &disk_config).await?;

        disk.path = Some(device.device_path);
        disk.oci_image_ref = None;
        disk.registry_url = None;
        mapped = true;
    }
    Ok(mapped)
}

/// Whether an overlaybd-tcmu process is running on this node.
async fn tcmu_daemon_running() -> bool {
    let Ok(mut entries) = tokio::fs::read_dir("/proc").await else {
        return false;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(comm) = tokio::fs::read_to_string(entry.path().join("comm")).await
            && comm.trim_end() == "overlaybd-tcmu"
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Maps every image to `/dev/fake-<vm>` and keeps the configs it was given.
    #[derive(Default)]
    struct FakeBackend {
        mapped: Mutex<Vec<serde_json::Value>>,
    }

    #[tonic::async_trait]
    impl StorageBackend for FakeBackend {
        async fn attach(&self, _: &str, _: &str) -> anyhow::Result<String> {
            unreachable!()
        }
        async fn detach(&self, _: &str, _: &str) -> anyhow::Result<()> {
            unreachable!()
        }
        async fn map(&self, vm_id: &str, config: &serde_json::Value) -> anyhow::Result<MappedDisk> {
            self.mapped.lock().unwrap().push(config.clone());
            Ok(MappedDisk {
                device_path: format!("/dev/fake-{vm_id}"),
            })
        }
        async fn unmap(&self, _: &str) -> anyhow::Result<()> {
            unreachable!()
        }
    }

    fn oci_disk() -> DiskConfig {
        DiskConfig {
            id: "rootfs".into(),
            oci_image_ref: Some("registry:5000/test/alpine:latest".into()),
            registry_url: Some("http://registry:5000".into()),
            upper_data_path: Some("/pool/upper.data".into()),
            upper_index_path: Some("/pool/upper.index".into()),
            ..Default::default()
        }
    }

    fn file_disk() -> DiskConfig {
        DiskConfig {
            id: "data".into(),
            path: Some("/pool/data.raw".into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn an_oci_disk_becomes_its_overlaybd_device() {
        let fake = Arc::new(FakeBackend::default());
        let backend: Arc<dyn StorageBackend> = fake.clone();
        let mut disks = vec![oci_disk(), file_disk()];

        assert!(
            map_oci_disks(Some(&backend), "vm1", &mut disks)
                .await
                .unwrap()
        );

        assert_eq!(disks[0].path.as_deref(), Some("/dev/fake-vm1"));
        assert_eq!(disks[0].oci_image_ref, None);
        assert_eq!(disks[0].registry_url, None);
        assert_eq!(disks[1], file_disk());
        let mapped = fake.mapped.lock().unwrap();
        assert_eq!(
            *mapped,
            vec![serde_json::json!({
                "image_ref": "registry:5000/test/alpine:latest",
                "registry_url": "http://registry:5000",
                "upper_data_path": "/pool/upper.data",
                "upper_index_path": "/pool/upper.index",
            })]
        );
    }

    #[tokio::test]
    async fn disks_without_an_image_need_no_backend() {
        let mut disks = vec![file_disk()];
        assert!(!map_oci_disks(None, "vm1", &mut disks).await.unwrap());
        assert_eq!(disks, vec![file_disk()]);
    }

    #[tokio::test]
    async fn an_oci_disk_without_an_overlaybd_backend_is_refused() {
        let error = map_oci_disks(None, "vm1", &mut [oci_disk()])
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("no OverlayBD backend"),
            "{error}"
        );
    }
}
