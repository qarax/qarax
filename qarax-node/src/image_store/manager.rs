use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flate2::read::GzDecoder;
use oci_client::Reference;
use oci_client::client::{Client, ClientConfig, ClientProtocol};
use oci_client::manifest::OciImageManifest;
use oci_client::secrets::RegistryAuth;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum ImageStoreError {
    #[error("OCI pull error: {0}")]
    PullError(#[from] oci_client::errors::OciDistributionError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Invalid image reference: {0}")]
    InvalidRef(String),

    #[error("virtiofsd failed to start: {0}")]
    VirtiofsdError(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageInfo {
    pub image_ref: String,
    pub digest: String,
    pub rootfs_path: PathBuf,
    // Store image configuration for VM booting
    pub env: Option<Vec<String>>,
    pub entrypoint: Option<Vec<String>>,
    pub cmd: Option<Vec<String>>,
}

// OCI Config parsing structures
#[derive(Serialize, Deserialize)]
struct OciImageConfig {
    config: OciImageConfigDetails,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OciImageConfigDetails {
    env: Option<Vec<String>>,
    entrypoint: Option<Vec<String>>,
    cmd: Option<Vec<String>>,
}

struct VirtiofsdProcess {
    _child: Child,
    socket_path: PathBuf,
}

pub struct ImageStoreManager {
    virtiofsd_binary: PathBuf,
    qarax_init_binary: PathBuf,
    cache_dir: PathBuf,
    runtime_dir: PathBuf,
    processes: Arc<Mutex<HashMap<String, VirtiofsdProcess>>>,
}

impl ImageStoreManager {
    pub fn new(
        virtiofsd_binary: impl Into<PathBuf>,
        qarax_init_binary: impl Into<PathBuf>,
        cache_dir: impl Into<PathBuf>,
        runtime_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            virtiofsd_binary: virtiofsd_binary.into(),
            qarax_init_binary: qarax_init_binary.into(),
            cache_dir: cache_dir.into(),
            runtime_dir: runtime_dir.into(),
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn safe_name(image_ref: &str) -> String {
        image_ref
            .chars()
            .map(|c| if matches!(c, '/' | ':' | '@') { '_' } else { c })
            .collect()
    }

    pub async fn pull_and_unpack(&self, image_ref: &str) -> Result<ImageInfo, ImageStoreError> {
        let safe_name = Self::safe_name(image_ref);
        let image_dir = self.cache_dir.join(&safe_name);
        let rootfs_dir = image_dir.join("rootfs");
        let digest_file = image_dir.join("digest.txt");

        // Return cached result if rootfs is already populated
        if rootfs_dir.exists() {
            let has_entries = match tokio::fs::read_dir(&rootfs_dir).await {
                Ok(mut rd) => rd.next_entry().await.ok().flatten().is_some(),
                Err(_) => false,
            };
            if has_entries {
                let digest = tokio::fs::read_to_string(&digest_file)
                    .await
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                let config_file = image_dir.join("config.json");
                let (env, entrypoint, cmd) = if config_file.exists() {
                    if let Ok(config_str) = tokio::fs::read_to_string(&config_file).await {
                        if let Ok(config) =
                            serde_json::from_str::<OciImageConfigDetails>(&config_str)
                        {
                            (config.env, config.entrypoint, config.cmd)
                        } else {
                            (None, None, None)
                        }
                    } else {
                        (None, None, None)
                    }
                } else {
                    (None, None, None)
                };

                info!("Using cached rootfs for {}", image_ref);
                return Ok(ImageInfo {
                    image_ref: image_ref.to_string(),
                    digest,
                    rootfs_path: rootfs_dir,
                    env,
                    entrypoint,
                    cmd,
                });
            }
        }

        tokio::fs::create_dir_all(&rootfs_dir).await?;

        let reference = Reference::try_from(image_ref)
            .map_err(|e| ImageStoreError::InvalidRef(e.to_string()))?;

        let mut insecure_hosts: Vec<String> =
            vec!["localhost".to_string(), "127.0.0.1".to_string()];
        if let Ok(val) = std::env::var("INSECURE_REGISTRIES") {
            for host in val.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                insecure_hosts.push(host.to_string());
            }
        }
        let config = ClientConfig {
            protocol: ClientProtocol::HttpsExcept(insecure_hosts),
            ..Default::default()
        };
        let client = Client::new(config);

        info!("Pulling manifest for {}", image_ref);
        let (manifest, digest): (OciImageManifest, String) = client
            .pull_image_manifest(&reference, &RegistryAuth::Anonymous)
            .await?;

        let mut config_buf: Vec<u8> = Vec::new();
        client
            .pull_blob(&reference, &manifest.config, &mut config_buf)
            .await?;
        let config_str = String::from_utf8(config_buf)
            .map_err(|e| ImageStoreError::VirtiofsdError(e.to_string()))?;
        let oci_config: OciImageConfig = serde_json::from_str(&config_str)?;

        let config_file = image_dir.join("config.json");
        tokio::fs::write(&config_file, serde_json::to_string(&oci_config.config)?).await?;

        for layer in &manifest.layers {
            let media_type = layer.media_type.as_str();
            let is_gzip = matches!(
                media_type,
                "application/vnd.oci.image.layer.v1.tar+gzip"
                    | "application/vnd.docker.image.rootfs.diff.tar.gzip"
            );
            let is_tar = media_type == "application/vnd.oci.image.layer.v1.tar";

            if !is_gzip && !is_tar {
                warn!("Skipping layer with unsupported media type: {}", media_type);
                continue;
            }

            let digest_prefix_len = std::cmp::min(16, layer.digest.len());
            info!(
                "Pulling layer {} ({})",
                &layer.digest[..digest_prefix_len],
                media_type
            );

            let mut buf: Vec<u8> = Vec::new();
            client.pull_blob(&reference, layer, &mut buf).await?;

            let unpack_dir = rootfs_dir.clone();
            tokio::task::spawn_blocking(move || {
                let cursor = Cursor::new(buf);
                if is_gzip {
                    let decoder = GzDecoder::new(cursor);
                    tar::Archive::new(decoder).unpack(&unpack_dir)
                } else {
                    tar::Archive::new(cursor).unpack(&unpack_dir)
                }
            })
            .await
            .map_err(|e| ImageStoreError::Io(std::io::Error::other(e)))?
            .map_err(ImageStoreError::Io)?;
        }

        tokio::fs::write(&digest_file, digest.as_bytes()).await?;

        info!("Unpacked {} to {}", image_ref, rootfs_dir.display());
        Ok(ImageInfo {
            image_ref: image_ref.to_string(),
            digest,
            rootfs_path: rootfs_dir,
            env: oci_config.config.env,
            entrypoint: oci_config.config.entrypoint,
            cmd: oci_config.config.cmd,
        })
    }

    pub async fn start_virtiofsd(
        &self,
        vm_id: &str,
        rootfs_dir: &Path,
    ) -> Result<PathBuf, ImageStoreError> {
        let socket_path = self.runtime_dir.join(format!("{}-fs.sock", vm_id));

        // Create overlayfs structure
        let vm_dir = self.runtime_dir.join(vm_id);
        let upper_dir = vm_dir.join("upper");
        let work_dir = vm_dir.join("work");
        let merged_dir = vm_dir.join("merged");

        tokio::fs::create_dir_all(&upper_dir).await?;
        tokio::fs::create_dir_all(&work_dir).await?;
        tokio::fs::create_dir_all(&merged_dir).await?;

        // Mount the overlay
        let mount_options = format!(
            "lowerdir={},upperdir={},workdir={}",
            rootfs_dir.to_str().unwrap_or_default(),
            upper_dir.to_str().unwrap_or_default(),
            work_dir.to_str().unwrap_or_default()
        );

        // We use mount here directly. We don't check if it's already mounted because
        // `mount` will just fail benignly or we assume a clean slate for now.
        // It's safer to unmount if it exists first just in case
        let _ = Command::new("umount").arg(&merged_dir).status().await;

        let mount_status = Command::new("mount")
            .args(["-t", "overlay", "overlay", "-o", &mount_options])
            .arg(&merged_dir)
            .status()
            .await
            .map_err(|e| {
                ImageStoreError::VirtiofsdError(format!("Failed to execute mount: {e}"))
            })?;

        if !mount_status.success() {
            return Err(ImageStoreError::VirtiofsdError(format!(
                "Failed to mount overlayfs for vm {vm_id}"
            )));
        }

        info!("OverlayFS mounted at {}", merged_dir.display());

        // Inject the qarax-init binary and its config into the overlay upper dir.
        if let Some(parent) = rootfs_dir.parent() {
            let config_file = parent.join("config.json");
            if config_file.exists()
                && let Ok(config_str) = tokio::fs::read_to_string(&config_file).await
                && let Ok(oci_config) = serde_json::from_str::<OciImageConfigDetails>(&config_str)
            {
                // Write /.qarax-config.json for the init binary to read at boot
                let init_config = serde_json::json!({
                    "entrypoint": oci_config.entrypoint.unwrap_or_default(),
                    "cmd": oci_config.cmd.unwrap_or_default(),
                    "env": oci_config.env.unwrap_or_default(),
                });
                let config_path = upper_dir.join(".qarax-config.json");
                tokio::fs::write(&config_path, init_config.to_string())
                    .await
                    .map_err(|e| {
                        ImageStoreError::VirtiofsdError(format!(
                            "failed to write .qarax-config.json: {e}"
                        ))
                    })?;

                // Copy the pre-built static qarax-init binary to /.qarax-init
                let init_dest = upper_dir.join(".qarax-init");
                tokio::fs::copy(&self.qarax_init_binary, &init_dest)
                    .await
                    .map_err(|e| {
                        ImageStoreError::VirtiofsdError(format!(
                            "failed to copy qarax-init from {}: {e} — is it installed?",
                            self.qarax_init_binary.display()
                        ))
                    })?;

                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(&init_dest, std::fs::Permissions::from_mode(0o755))
                    .await
                    .map_err(|e| {
                        ImageStoreError::VirtiofsdError(format!(
                            "failed to chmod +x .qarax-init: {e}"
                        ))
                    })?;

                info!("Injected qarax-init binary at {}", init_dest.display());
            }
        }

        // Remove stale socket
        if socket_path.exists() {
            let _ = tokio::fs::remove_file(&socket_path).await;
        }

        let child = Command::new(&self.virtiofsd_binary)
            .args([
                "--socket-path",
                socket_path.to_str().unwrap_or_default(),
                "--shared-dir",
                merged_dir.to_str().unwrap_or_default(),
                "--cache=auto",
            ])
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ImageStoreError::VirtiofsdError(format!("spawn failed: {e}")))?;

        // Wait for socket to appear
        let mut waited = 0u32;
        loop {
            if socket_path.exists() {
                break;
            }
            if waited >= 50 {
                return Err(ImageStoreError::VirtiofsdError(format!(
                    "socket {} did not appear after 5s",
                    socket_path.display()
                )));
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            waited += 1;
        }

        info!(
            "virtiofsd started for {} at {}",
            vm_id,
            socket_path.display()
        );
        self.processes.lock().await.insert(
            vm_id.to_string(),
            VirtiofsdProcess {
                _child: child,
                socket_path: socket_path.clone(),
            },
        );

        Ok(socket_path)
    }

    pub async fn stop_virtiofsd(&self, vm_id: &str) {
        let mut processes = self.processes.lock().await;
        if let Some(proc) = processes.remove(vm_id) {
            let _ = tokio::fs::remove_file(&proc.socket_path).await;
            // child dropped here → killed due to kill_on_drop(true)
        }
    }

    pub async fn get_image_status(&self, image_ref: &str) -> Option<ImageInfo> {
        let safe_name = Self::safe_name(image_ref);
        let image_dir = self.cache_dir.join(&safe_name);
        let rootfs_dir = image_dir.join("rootfs");

        if !rootfs_dir.exists() {
            return None;
        }

        let has_entries = match tokio::fs::read_dir(&rootfs_dir).await {
            Ok(mut rd) => rd.next_entry().await.ok().flatten().is_some(),
            Err(_) => false,
        };
        if !has_entries {
            return None;
        }

        let digest = tokio::fs::read_to_string(image_dir.join("digest.txt"))
            .await
            .unwrap_or_default()
            .trim()
            .to_string();

        let config_file = image_dir.join("config.json");
        let (env, entrypoint, cmd) = if config_file.exists() {
            if let Ok(config_str) = tokio::fs::read_to_string(&config_file).await {
                if let Ok(config) = serde_json::from_str::<OciImageConfigDetails>(&config_str) {
                    (config.env, config.entrypoint, config.cmd)
                } else {
                    (None, None, None)
                }
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        };

        Some(ImageInfo {
            image_ref: image_ref.to_string(),
            digest,
            rootfs_path: rootfs_dir,
            env,
            entrypoint,
            cmd,
        })
    }

    pub async fn cleanup_vm(&self, vm_id: &str) {
        if self.processes.lock().await.contains_key(vm_id) {
            self.stop_virtiofsd(vm_id).await;
        }

        // Clean up overlayfs
        let vm_dir = self.runtime_dir.join(vm_id);
        let merged_dir = vm_dir.join("merged");

        if merged_dir.exists() {
            let _ = Command::new("umount").arg(&merged_dir).status().await;
        }

        if vm_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&vm_dir).await;
        }
    }
}
