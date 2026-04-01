//! OverlayBD manager for lazy block-level OCI image loading.
//!
//! This module manages the lifecycle of OverlayBD block devices:
//! - Copying OCI images to the local registry via `oci-client`
//! - Converting the copied image to OverlayBD format via `convertor`
//! - Mounting images as block devices via overlaybd-tcmu + tcm_loop loopback
//! - Unmounting by tearing down the loopback fabric and TCMU backstore

use std::collections::HashMap;
use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use oci_client::Reference;
use oci_client::client::{Client, ClientConfig, ClientProtocol, Config, ImageLayer};
use oci_client::secrets::RegistryAuth;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::image_preflight::{
    PreflightCheckResult, architecture_check, boot_mode_check, guest_command_check,
};
use crate::oci_config::{OciImageConfig, OciImageConfigDetails};

/// Path to the overlaybd-create binary installed by the overlaybd RPM.
const OVERLAYBD_CREATE: &str = "/opt/overlaybd/bin/overlaybd-create";

/// Virtual disk size presented to Cloud Hypervisor (sparse, so barely uses space).
const DISK_SIZE_GB: u64 = 64;
const DISK_SIZE_BYTES: u64 = DISK_SIZE_GB * 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum OverlayBdError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("OCI registry error: {0}")]
    OciError(String),

    #[error("convertor failed: {0}")]
    ConvertorFailed(String),

    #[error("Mount timed out waiting for TCMU device to become LIVE")]
    MountTimeout,

    #[error("Mount failed: {0}")]
    MountFailed(String),

    #[error("Invalid image reference: {0}")]
    InvalidImageRef(String),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Normalize a registry URL to a bare host[:port] string.
fn registry_host(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

/// Represents a successfully mounted OverlayBD block device.
#[derive(Debug, Clone)]
pub struct MountedDevice {
    /// Host block device path inside the container, e.g. "/dev/sdb"
    pub device_path: String,
    /// Per-VM config directory (removed on unmount)
    pub config_dir: PathBuf,
    /// TCMU backstore name in configfs (derived from vm_id)
    tcmu_name: String,
    /// Loopback target WWN in configfs (derived from vm_id)
    wwn: String,
}

/// Manages OverlayBD block devices for VMs.
pub struct OverlayBdManager {
    /// Path to the `convertor` binary (from accelerated-container-image)
    convertor_binary: PathBuf,
    /// Base cache directory, default: /var/lib/qarax/overlaybd/
    cache_dir: PathBuf,
    /// Currently mounted devices, keyed by VM ID
    mounts: Arc<Mutex<HashMap<String, MountedDevice>>>,
}

impl OverlayBdManager {
    pub fn new(convertor_binary: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        let convertor_binary = convertor_binary.into();
        let cache_dir = cache_dir.into();
        info!(
            "OverlayBdManager initialized: convertor={}, cache_dir={}",
            convertor_binary.display(),
            cache_dir.display()
        );
        Self {
            convertor_binary,
            cache_dir,
            mounts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Import an OCI image into the local OverlayBD storage pool:
    ///   1. Copy the source OCI image to the local registry using `oci-client`.
    ///   2. Convert the mirrored image to OverlayBD format using `convertor`.
    ///
    /// Returns the image reference in the target registry.
    pub async fn import_image(
        &self,
        image_ref: &str,
        registry_url: &str,
    ) -> Result<String, OverlayBdError> {
        let target_ref = build_target_ref(image_ref, registry_url)?;

        info!("Copying OCI image {} → {}", image_ref, target_ref);
        self.copy_image(image_ref, &target_ref, registry_url)
            .await?;

        info!("Converting {} to OverlayBD format", target_ref);
        self.convert_to_overlaybd(&target_ref).await?;

        info!("OverlayBD image ready: {}", target_ref);
        Ok(target_ref)
    }

    /// Copy an OCI image from an arbitrary source registry to the local registry.
    async fn copy_image(
        &self,
        source: &str,
        target: &str,
        registry_url: &str,
    ) -> Result<(), OverlayBdError> {
        let source_ref = Reference::try_from(source)
            .map_err(|e| OverlayBdError::InvalidImageRef(e.to_string()))?;
        let target_ref = Reference::try_from(target)
            .map_err(|e| OverlayBdError::InvalidImageRef(e.to_string()))?;

        let mut excepts = vec![registry_host(registry_url)];
        let source_host = source_ref.registry().to_string();
        if source_host.starts_with("localhost") || source_host.starts_with("127.0.0.1") {
            excepts.push(source_host);
        }

        let client = Client::new(ClientConfig {
            protocol: ClientProtocol::HttpsExcept(excepts),
            ..Default::default()
        });

        let (manifest, _digest) = client
            .pull_image_manifest(&source_ref, &RegistryAuth::Anonymous)
            .await
            .map_err(|e| OverlayBdError::OciError(e.to_string()))?;

        let mut config_data: Vec<u8> = Vec::new();
        client
            .pull_blob(&source_ref, &manifest.config, &mut config_data)
            .await
            .map_err(|e| OverlayBdError::OciError(e.to_string()))?;

        let config = Config {
            data: config_data.into(),
            media_type: manifest.config.media_type.clone(),
            annotations: manifest.config.annotations.clone(),
        };

        let mut image_layers: Vec<ImageLayer> = Vec::new();
        for layer_desc in &manifest.layers {
            let mut buf: Vec<u8> = Vec::new();
            client
                .pull_blob(&source_ref, layer_desc, &mut buf)
                .await
                .map_err(|e| OverlayBdError::OciError(e.to_string()))?;
            info!(
                "Pulled layer {} ({} bytes)",
                &layer_desc.digest[..std::cmp::min(16, layer_desc.digest.len())],
                buf.len()
            );
            image_layers.push(ImageLayer {
                data: buf.into(),
                media_type: layer_desc.media_type.clone(),
                annotations: layer_desc.annotations.clone(),
            });
        }

        client
            .push(
                &target_ref,
                &image_layers,
                config,
                &RegistryAuth::Anonymous,
                Some(manifest),
            )
            .await
            .map_err(|e| OverlayBdError::OciError(e.to_string()))?;

        info!("Copied OCI image to {}", target);
        Ok(())
    }

    /// Convert the image at `target_ref` to OverlayBD format in-place.
    async fn convert_to_overlaybd(&self, target_ref: &str) -> Result<(), OverlayBdError> {
        let reference = Reference::try_from(target_ref)
            .map_err(|e| OverlayBdError::InvalidImageRef(e.to_string()))?;

        let registry = reference.registry();
        let repository = reference.repository();
        let tag = reference.tag().unwrap_or("latest");

        let output = tokio::process::Command::new(&self.convertor_binary)
            .args([
                "--repository",
                &format!("{}/{}", registry, repository),
                "--input-tag",
                tag,
                "--overlaybd",
                tag,
                "--plain",
            ])
            .output()
            .await
            .map_err(|e| {
                OverlayBdError::ConvertorFailed(format!("failed to spawn convertor: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(OverlayBdError::ConvertorFailed(format!(
                "convertor exited with {}\nstdout: {}\nstderr: {}",
                output.status, stdout, stderr
            )));
        }

        Ok(())
    }

    /// Mount an OverlayBD image as a block device for the given VM.
    ///
    /// Full flow:
    ///   1. Create writable upper layer files via `overlaybd-create`
    ///   2. Write the overlaybd TCMU config.json
    ///   3. Create a TCMU backstore in configfs and enable it
    ///   4. Wait for overlaybd-tcmu daemon to reach LIVE state
    ///   5. Create a tcm_loop loopback fabric target backed by the TCMU backstore
    ///   6. Find the resulting SCSI block device, create its node in /dev/, return it
    pub async fn mount(
        &self,
        vm_id: &str,
        image_ref: &str,
        registry_url: &str,
        upper_data_path: Option<&str>,
        upper_index_path: Option<&str>,
    ) -> Result<MountedDevice, OverlayBdError> {
        let config_dir = self.cache_dir.join(vm_id);
        tokio::fs::create_dir_all(&config_dir).await.map_err(|e| {
            OverlayBdError::MountFailed(format!("create cache dir {}: {}", config_dir.display(), e))
        })?;

        // Use caller-supplied persistent paths when provided; otherwise fall
        // back to ephemeral paths inside the config_dir (deleted on unmount).
        let upper_index = upper_index_path
            .map(PathBuf::from)
            .unwrap_or_else(|| config_dir.join("upper.index"));
        let upper_data = upper_data_path
            .map(PathBuf::from)
            .unwrap_or_else(|| config_dir.join("upper.data"));
        let config_file = config_dir.join("config.json");

        // Build the registry blob URL used by overlaybd to fetch image data lazily.
        let (repo_name, _tag) = parse_image_ref(image_ref)?;
        let repo_blob_url = format!(
            "{}/v2/{}/blobs/",
            registry_url.trim_end_matches('/'),
            repo_name
        );

        // 1. Create the writable upper layer (idempotent: skip if already exists).
        if !upper_data.exists() {
            // For persistent upper layers the parent directory (pool path) may
            // not exist yet on this host — create it if needed.
            if let Some(parent) = upper_data.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    OverlayBdError::MountFailed(format!(
                        "create upper layer dir {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
            info!(
                "Creating OverlayBD upper layer for VM {} ({} GB sparse)",
                vm_id, DISK_SIZE_GB
            );
            let out = tokio::process::Command::new(OVERLAYBD_CREATE)
                .args([
                    upper_data.to_str().unwrap_or(""),
                    upper_index.to_str().unwrap_or(""),
                    &DISK_SIZE_GB.to_string(),
                ])
                .output()
                .await
                .map_err(|e| {
                    OverlayBdError::MountFailed(format!("exec {}: {}", OVERLAYBD_CREATE, e))
                })?;
            if !out.status.success() {
                return Err(OverlayBdError::MountFailed(format!(
                    "overlaybd-create failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
        }

        // 2. Write the overlaybd TCMU config.json.
        //    Fetch the manifest to build the lowers array (OverlayBD layer descriptors).
        //    overlaybd-tcmu needs these to lazy-fetch image data from the registry.
        let lowers = self.fetch_lowers(image_ref, registry_url).await?;
        let config_json = serde_json::json!({
            "repoBlobUrl": repo_blob_url,
            "lowers": lowers,
            "upper": {
                "index": upper_index.to_string_lossy(),
                "data":  upper_data.to_string_lossy()
            },
            "resultFile": config_dir.join("result").to_string_lossy()
        });
        tokio::fs::write(&config_file, serde_json::to_string_pretty(&config_json)?)
            .await
            .map_err(|e| {
                OverlayBdError::MountFailed(format!(
                    "write config {}: {}",
                    config_file.display(),
                    e
                ))
            })?;

        info!(
            "Wrote OverlayBD TCMU config for VM {} at {}",
            vm_id,
            config_file.display()
        );

        // 3. Create the TCMU backstore in configfs.
        //    Names are derived deterministically from vm_id so they survive restart.
        let (tcmu_name, wwn) = Self::vm_tcmu_names(vm_id);

        let tcmu_base = PathBuf::from("/sys/kernel/config/target/core/user_1");
        let tcmu_dir = tcmu_base.join(&tcmu_name);

        // Ensure the TCMU handler directory exists in configfs (not auto-created
        // by the kernel module — must be mkdir'd to register the handler).
        if !tcmu_base.exists() {
            tokio::fs::create_dir(&tcmu_base).await.map_err(|e| {
                OverlayBdError::MountFailed(format!(
                    "create TCMU handler dir {}: {}",
                    tcmu_base.display(),
                    e
                ))
            })?;
        }

        // Remove stale backstore from a previous (failed) attempt if present.
        if tcmu_dir.exists() {
            let _ = tokio::fs::write(tcmu_dir.join("enable"), "0\n").await;
            let _ = tokio::fs::remove_dir(&tcmu_dir).await;
        }

        tokio::fs::create_dir(&tcmu_dir).await.map_err(|e| {
            OverlayBdError::MountFailed(format!(
                "create TCMU backstore dir {}: {}",
                tcmu_dir.display(),
                e
            ))
        })?;

        self.wait_for_tcmu_control_files(&tcmu_dir).await?;

        let dev_config = format!(
            "dev_config=overlaybd/{},dev_size={},dev_max_sectors=128",
            config_file.display(),
            DISK_SIZE_BYTES
        );
        tokio::fs::write(tcmu_dir.join("control"), &dev_config)
            .await
            .map_err(|e| {
                OverlayBdError::MountFailed(format!(
                    "write TCMU control {}: {}",
                    tcmu_dir.join("control").display(),
                    e
                ))
            })?;

        tokio::fs::write(tcmu_dir.join("enable"), "1\n")
            .await
            .map_err(|e| {
                OverlayBdError::MountFailed(format!(
                    "write TCMU enable {}: {}",
                    tcmu_dir.join("enable").display(),
                    e
                ))
            })?;

        info!(
            "TCMU backstore '{}' enabled with config: {}",
            tcmu_name, dev_config
        );

        // 4. Wait for overlaybd-tcmu daemon to open the device.
        //    The TCMU backstore stays in DEACTIVATED state until a loopback fabric
        //    LUN links to it, so we can't wait for LIVE here.  Instead, verify
        //    that the daemon's dev_open callback succeeded by checking that
        //    hw_block_size has been set (non-zero means the kernel processed the
        //    enable and the daemon responded).
        self.wait_for_tcmu_configured(&tcmu_dir).await?;

        // 5 & 6. Set up loopback fabric and obtain the block device path.
        let device_path = self
            .setup_loopback_and_get_device(&wwn, &tcmu_name, &tcmu_dir)
            .await?;

        info!("OverlayBD device for VM {}: {}", vm_id, device_path);

        let mounted = MountedDevice {
            device_path,
            config_dir: config_dir.clone(),
            tcmu_name,
            wwn,
        };

        self.mounts
            .lock()
            .await
            .insert(vm_id.to_string(), mounted.clone());

        Ok(mounted)
    }

    pub async fn preflight_boot(
        &self,
        image_ref: &str,
        registry_url: &str,
        architecture: &str,
        boot_mode: &str,
        qarax_init_binary: Option<&Path>,
    ) -> Result<(String, Vec<PreflightCheckResult>), OverlayBdError> {
        let mut checks = vec![boot_mode_check(boot_mode)];

        let imported_ref = self.import_image(image_ref, registry_url).await?;
        checks.push(PreflightCheckResult::ok(
            "overlaybd_import",
            format!("imported image into local registry as {}", imported_ref),
        ));

        let oci_config = self
            .fetch_full_oci_config(&imported_ref, registry_url)
            .await?;
        let manifest_architecture = oci_config
            .architecture
            .as_deref()
            .and_then(common::architecture::normalize_architecture)
            .unwrap_or_else(|| architecture.to_string());
        checks.push(architecture_check(architecture, &manifest_architecture));
        match oci_config.os.as_deref() {
            Some("linux") | None => checks.push(PreflightCheckResult::ok(
                "os",
                format!(
                    "image OS is {}",
                    oci_config.os.as_deref().unwrap_or("linux (unspecified)")
                ),
            )),
            Some(os) => checks.push(PreflightCheckResult::fail(
                "os",
                format!("image OS {} is not supported for VM boot", os),
            )),
        }

        let vm_id = format!("preflight-{}", Uuid::new_v4());
        let mount_result = self
            .mount(&vm_id, &imported_ref, registry_url, None, None)
            .await;
        let mounted = match mount_result {
            Ok(mounted) => mounted,
            Err(e) => {
                checks.push(PreflightCheckResult::fail("overlaybd_mount", e.to_string()));
                self.unmount(&vm_id).await;
                return Ok((imported_ref, checks));
            }
        };

        checks.push(PreflightCheckResult::ok(
            "overlaybd_mount",
            format!("mapped OverlayBD device at {}", mounted.device_path),
        ));

        let mount_dir = self.cache_dir.join(&vm_id).join("rootfs-mount");
        tokio::fs::create_dir_all(&mount_dir).await.map_err(|e| {
            OverlayBdError::MountFailed(format!("create mount dir {}: {}", mount_dir.display(), e))
        })?;

        let out = tokio::process::Command::new("mount")
            .args([
                mounted.device_path.as_str(),
                mount_dir.to_str().unwrap_or(""),
            ])
            .output()
            .await
            .map_err(|e| OverlayBdError::MountFailed(format!("exec mount: {}", e)))?;

        if !out.status.success() {
            checks.push(PreflightCheckResult::fail(
                "rootfs_mount",
                format!(
                    "mount {} on {}: {}",
                    mounted.device_path,
                    mount_dir.display(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            ));
            let _ = tokio::fs::remove_dir(&mount_dir).await;
            self.unmount(&vm_id).await;
            return Ok((imported_ref, checks));
        }

        checks.push(guest_command_check(&oci_config.config, &mount_dir).await);

        let init_result = match qarax_init_binary {
            Some(path) if path.exists() => {
                match self
                    .inject_init_inner(&mount_dir, &imported_ref, registry_url, path)
                    .await
                {
                    Ok(()) => PreflightCheckResult::ok(
                        "qarax_init",
                        format!("injected qarax-init from {}", path.display()),
                    ),
                    Err(e) => PreflightCheckResult::fail("qarax_init", e.to_string()),
                }
            }
            Some(path) => PreflightCheckResult::fail(
                "qarax_init",
                format!("qarax-init binary is missing at {}", path.display()),
            ),
            None => PreflightCheckResult::fail(
                "qarax_init",
                "qarax-init binary is not configured on this node",
            ),
        };
        checks.push(init_result);

        let _ = tokio::process::Command::new("umount")
            .arg(mount_dir.to_str().unwrap_or(""))
            .output()
            .await;
        let _ = tokio::fs::remove_dir(&mount_dir).await;
        self.unmount(&vm_id).await;

        Ok((imported_ref, checks))
    }

    /// Unmount the OverlayBD device for the given VM:
    ///   1. Tear down the loopback fabric (disable TPG, remove LUN symlink, remove dirs)
    ///   2. Disable and remove the TCMU backstore
    ///   3. Remove the per-VM config directory
    pub async fn unmount(&self, vm_id: &str) {
        let mount_info = {
            let mut mounts = self.mounts.lock().await;
            mounts.remove(vm_id)
        };

        let (config_dir, tcmu_name, wwn) = if let Some(m) = mount_info {
            (m.config_dir, m.tcmu_name, m.wwn)
        } else {
            // Re-derive names deterministically (no files needed).
            let (tcmu_name, wwn) = Self::vm_tcmu_names(vm_id);
            (self.cache_dir.join(vm_id), tcmu_name, wwn)
        };

        // Tear down loopback fabric.
        let loopback_dir = PathBuf::from(format!("/sys/kernel/config/target/loopback/{}", wwn));
        if loopback_dir.exists() {
            let tpg_dir = loopback_dir.join("tpgt_1");
            // Disable the TPG first.
            let _ = tokio::fs::write(tpg_dir.join("enable"), "0\n").await;
            // Remove the LUN symlink.
            let lun_dir = tpg_dir.join("lun").join("lun_0");
            if let Ok(mut rd) = tokio::fs::read_dir(&lun_dir).await {
                while let Ok(Some(e)) = rd.next_entry().await {
                    let _ = tokio::fs::remove_file(e.path()).await;
                }
            }
            // Remove directories in reverse order.
            for dir in &[
                lun_dir.clone(),
                tpg_dir.join("lun"),
                tpg_dir.clone(),
                loopback_dir.clone(),
            ] {
                if let Err(e) = tokio::fs::remove_dir(dir).await {
                    warn!("Failed to remove {}: {}", dir.display(), e);
                }
            }
        }

        // Tear down the TCMU backstore.
        let tcmu_dir = PathBuf::from(format!(
            "/sys/kernel/config/target/core/user_1/{}",
            tcmu_name
        ));
        if tcmu_dir.exists() {
            let _ = tokio::fs::write(tcmu_dir.join("enable"), "0\n").await;
            if let Err(e) = tokio::fs::remove_dir(&tcmu_dir).await {
                warn!(
                    "Failed to remove TCMU backstore dir {}: {}",
                    tcmu_dir.display(),
                    e
                );
            }
        }

        // Remove the per-VM config directory.
        if config_dir.exists() {
            if let Err(e) = tokio::fs::remove_dir_all(&config_dir).await {
                warn!(
                    "Failed to remove OverlayBD config dir for VM {}: {}",
                    vm_id, e
                );
            } else {
                info!("OverlayBD config dir removed for VM {}", vm_id);
            }
        }
    }

    /// Scan the cache directory on startup and rebuild the mounts map from any
    /// existing result files left over from a previous run.
    pub async fn recover(&self) {
        let mut read_dir = match tokio::fs::read_dir(&self.cache_dir).await {
            Ok(rd) => rd,
            Err(_) => return,
        };

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let vm_id = match entry.file_name().into_string() {
                Ok(s) => s,
                Err(_) => continue,
            };

            let result_file = entry.path().join("result");
            if !result_file.exists() {
                continue;
            }

            let device_path = match tokio::fs::read_to_string(&result_file).await {
                Ok(s) => s.trim().to_string(),
                Err(_) => continue,
            };

            if !looks_like_device_path(&device_path) {
                warn!(
                    "Ignoring stale OverlayBD recovery state for VM {}: invalid result '{}'",
                    vm_id, device_path
                );
                self.unmount(&vm_id).await;
                continue;
            }

            let (tcmu_name, wwn) = Self::vm_tcmu_names(&vm_id);

            info!(
                "Recovered OverlayBD mount for VM {}: {}",
                vm_id, device_path
            );

            let mounted = MountedDevice {
                device_path,
                config_dir: entry.path(),
                tcmu_name,
                wwn,
            };
            self.mounts.lock().await.insert(vm_id, mounted);
        }
    }

    /// Inject the qarax-init binary and config into a mounted OverlayBD block device.
    ///
    /// The block device is temporarily mounted, the init binary and OCI config
    /// (entrypoint/cmd/env) are written to the root, then it's unmounted.
    /// The kernel cmdline should include `init=/.qarax-init` so PID 1 reads the config.
    pub async fn inject_init(
        &self,
        vm_id: &str,
        device_path: &str,
        image_ref: &str,
        registry_url: &str,
        qarax_init_binary: &Path,
    ) -> Result<(), OverlayBdError> {
        let mount_dir = self.cache_dir.join(vm_id).join("rootfs-mount");
        tokio::fs::create_dir_all(&mount_dir).await.map_err(|e| {
            OverlayBdError::MountFailed(format!("create mount dir {}: {}", mount_dir.display(), e))
        })?;

        // Mount the block device
        let out = tokio::process::Command::new("mount")
            .args([device_path, mount_dir.to_str().unwrap_or("")])
            .output()
            .await
            .map_err(|e| OverlayBdError::MountFailed(format!("exec mount: {}", e)))?;

        if !out.status.success() {
            return Err(OverlayBdError::MountFailed(format!(
                "mount {} on {}: {}",
                device_path,
                mount_dir.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }

        // Use a closure-like approach to ensure we unmount even on error
        let result = self
            .inject_init_inner(&mount_dir, image_ref, registry_url, qarax_init_binary)
            .await;

        // Always unmount
        let umount_out = tokio::process::Command::new("umount")
            .arg(mount_dir.to_str().unwrap_or(""))
            .output()
            .await;

        match &umount_out {
            Ok(o) if !o.status.success() => {
                warn!(
                    "umount {} failed: {}",
                    mount_dir.display(),
                    String::from_utf8_lossy(&o.stderr).trim()
                );
            }
            Err(e) => warn!("exec umount: {}", e),
            _ => {}
        }

        // Clean up mount dir
        let _ = tokio::fs::remove_dir(&mount_dir).await;

        result
    }

    async fn inject_init_inner(
        &self,
        mount_dir: &Path,
        image_ref: &str,
        registry_url: &str,
        qarax_init_binary: &Path,
    ) -> Result<(), OverlayBdError> {
        // Fetch OCI config from the local registry to get entrypoint/cmd/env.
        // We need the *original* (non-overlaybd-converted) image's config.
        // The image_ref points to the local registry copy.
        let oci_config = self.fetch_oci_config(image_ref, registry_url).await?;

        // Write /.qarax-config.json
        let init_config = serde_json::json!({
            "entrypoint": oci_config.entrypoint.unwrap_or_default(),
            "cmd": oci_config.cmd.unwrap_or_default(),
            "env": oci_config.env.unwrap_or_default(),
        });
        let config_path = mount_dir.join(".qarax-config.json");
        tokio::fs::write(&config_path, init_config.to_string())
            .await
            .map_err(|e| OverlayBdError::MountFailed(format!("write .qarax-config.json: {}", e)))?;

        // Copy qarax-init binary to /.qarax-init
        let init_dest = mount_dir.join(".qarax-init");
        tokio::fs::copy(qarax_init_binary, &init_dest)
            .await
            .map_err(|e| {
                OverlayBdError::MountFailed(format!(
                    "copy qarax-init from {}: {} — is it installed?",
                    qarax_init_binary.display(),
                    e
                ))
            })?;

        tokio::fs::set_permissions(&init_dest, std::fs::Permissions::from_mode(0o755))
            .await
            .map_err(|e| OverlayBdError::MountFailed(format!("chmod +x .qarax-init: {}", e)))?;

        info!(
            "Injected qarax-init binary and config at {}",
            mount_dir.display()
        );
        Ok(())
    }

    /// Fetch the OCI image config (entrypoint/cmd/env) from the local registry.
    async fn fetch_oci_config(
        &self,
        image_ref: &str,
        registry_url: &str,
    ) -> Result<OciImageConfigDetails, OverlayBdError> {
        Ok(self
            .fetch_full_oci_config(image_ref, registry_url)
            .await?
            .config)
    }

    async fn fetch_full_oci_config(
        &self,
        image_ref: &str,
        registry_url: &str,
    ) -> Result<OciImageConfig, OverlayBdError> {
        let host = registry_host(registry_url);

        let full_ref_str = if image_ref.starts_with(&host) {
            image_ref.to_string()
        } else {
            format!("{}/{}", host, image_ref)
        };

        let local_ref = Reference::try_from(full_ref_str.as_str())
            .map_err(|e| OverlayBdError::InvalidImageRef(e.to_string()))?;

        let client = Client::new(ClientConfig {
            protocol: ClientProtocol::HttpsExcept(vec![host]),
            ..Default::default()
        });

        let (manifest, _digest) = client
            .pull_image_manifest(&local_ref, &RegistryAuth::Anonymous)
            .await
            .map_err(|e| {
                OverlayBdError::OciError(format!("fetch manifest for {}: {}", full_ref_str, e))
            })?;

        let mut config_data: Vec<u8> = Vec::new();
        client
            .pull_blob(&local_ref, &manifest.config, &mut config_data)
            .await
            .map_err(|e| {
                OverlayBdError::OciError(format!("fetch config blob for {}: {}", full_ref_str, e))
            })?;

        serde_json::from_slice(&config_data).map_err(|e| {
            OverlayBdError::OciError(format!("parse OCI config for {}: {}", full_ref_str, e))
        })
    }

    /// Fetch layer descriptors from the converted OverlayBD image manifest in the local registry.
    /// These populate the `lowers` array in the TCMU config.json so overlaybd-tcmu knows
    /// which blobs to lazy-fetch from the registry.
    async fn fetch_lowers(
        &self,
        image_ref: &str,
        registry_url: &str,
    ) -> Result<Vec<serde_json::Value>, OverlayBdError> {
        let host = registry_host(registry_url);

        // image_ref already includes the registry host (e.g. "registry:5000/docker/library/alpine:latest")
        let full_ref_str = if image_ref.starts_with(&host) {
            image_ref.to_string()
        } else {
            format!("{}/{}", host, image_ref)
        };

        let local_ref = Reference::try_from(full_ref_str.as_str())
            .map_err(|e| OverlayBdError::InvalidImageRef(e.to_string()))?;

        let client = Client::new(ClientConfig {
            protocol: ClientProtocol::HttpsExcept(vec![host]),
            ..Default::default()
        });

        let (manifest, _digest) = client
            .pull_image_manifest(&local_ref, &RegistryAuth::Anonymous)
            .await
            .map_err(|e| {
                OverlayBdError::OciError(format!(
                    "Failed to fetch manifest for {}: {}",
                    full_ref_str, e
                ))
            })?;

        if manifest.layers.is_empty() {
            return Err(OverlayBdError::MountFailed(format!(
                "Converted OverlayBD image {} has no layers in manifest",
                full_ref_str
            )));
        }

        let lowers = manifest
            .layers
            .iter()
            .map(|layer| {
                serde_json::json!({
                    "digest": layer.digest,
                    "size": layer.size,
                    "dir": ""
                })
            })
            .collect();

        Ok(lowers)
    }

    /// Configfs object creation is asynchronous enough that newly-created TCMU
    /// backstores can briefly exist before their control files appear. Wait for
    /// the writable control surface before writing `control` or `enable`.
    async fn wait_for_tcmu_control_files(&self, tcmu_dir: &Path) -> Result<(), OverlayBdError> {
        let control = tcmu_dir.join("control");
        let enable = tcmu_dir.join("enable");

        for _ in 0..50 {
            if control.exists() && enable.exists() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(OverlayBdError::MountFailed(format!(
            "TCMU control files did not appear under {}",
            tcmu_dir.display()
        )))
    }

    /// Wait for the TCMU backstore to be configured by the overlaybd-tcmu daemon.
    ///
    /// After writing `1` to `enable`, the kernel creates a UIO device and notifies
    /// the daemon.  The daemon calls `dev_open` which reads the image config.  Once
    /// the daemon responds, the kernel sets `hw_block_size` to a non-zero value
    /// (typically 512).  We poll for this as a lightweight check that the daemon
    /// successfully opened the device.
    ///
    /// Note: The backstore remains in DEACTIVATED state until a LUN symlink connects
    /// it to a fabric (loopback).  The ACTIVATED state only appears after that link.
    async fn wait_for_tcmu_configured(&self, tcmu_dir: &Path) -> Result<(), OverlayBdError> {
        let info_file = tcmu_dir.join("info");

        for _ in 0..100 {
            // up to 10 s
            tokio::time::sleep(Duration::from_millis(100)).await;

            match tokio::fs::read_to_string(&info_file).await {
                Ok(content) if content.contains("SectorSize: 512") => {
                    info!("TCMU backstore configured (sector size set)");
                    return Ok(());
                }
                Ok(content) if content.contains("Status: ACTIVATED") => {
                    return Ok(());
                }
                _ => {}
            }
        }

        let final_status = tokio::fs::read_to_string(&info_file)
            .await
            .unwrap_or_else(|_| "unable to read info file".into());

        // Emit the overlaybd daemon log to help diagnose the failure.
        let obd_log = tokio::fs::read_to_string("/var/log/overlaybd.log")
            .await
            .unwrap_or_else(|_| "(overlaybd.log not readable)".into());
        let tail: Vec<&str> = obd_log.lines().rev().take(40).collect();
        let tail_str: Vec<&str> = tail.into_iter().rev().collect();
        warn!(
            "overlaybd-tcmu log (last 40 lines):\n{}",
            tail_str.join("\n")
        );

        Err(OverlayBdError::MountFailed(format!(
            "TCMU backstore not configured after 10s. Last info: {}",
            final_status.lines().next().unwrap_or("").trim()
        )))
    }

    /// Set up a tcm_loop loopback fabric target backed by the given TCMU backstore,
    /// wait for the resulting SCSI block device to appear in sysfs, create its
    /// device node in the container's /dev/, and return the device path.
    async fn setup_loopback_and_get_device(
        &self,
        wwn: &str,
        tcmu_name: &str,
        tcmu_dir: &Path,
    ) -> Result<String, OverlayBdError> {
        // Record sd* devices that already exist so we can detect the new one.
        let known = self.list_sd_devices().await?;

        let loopback_base = PathBuf::from("/sys/kernel/config/target/loopback");
        let loopback_dir = loopback_base.join(wwn);
        let tpg_dir = loopback_dir.join("tpgt_1");
        let lun_dir = tpg_dir.join("lun").join("lun_0");

        // Create each configfs directory individually — configfs triggers kernel-side
        // object registration on each mkdir, so create_dir_all does NOT work here.
        for dir in &[
            &loopback_base,
            &loopback_dir,
            &tpg_dir,
            &tpg_dir.join("lun"),
            &lun_dir,
        ] {
            if !dir.exists() {
                tokio::fs::create_dir(dir).await.map_err(|e| {
                    OverlayBdError::MountFailed(format!(
                        "create configfs dir {}: {}",
                        dir.display(),
                        e
                    ))
                })?;
            }
        }

        // Write the nexus (initiator WWN = target WWN for loopback).
        // This creates the SCSI host adapter that connects initiator → target.
        tokio::fs::write(tpg_dir.join("nexus"), wwn)
            .await
            .map_err(|e| {
                OverlayBdError::MountFailed(format!(
                    "write loopback nexus {}: {}",
                    tpg_dir.join("nexus").display(),
                    e
                ))
            })?;

        // Symlink the LUN to the TCMU backstore.
        let tcmu_abs = tokio::fs::canonicalize(tcmu_dir).await.map_err(|e| {
            OverlayBdError::MountFailed(format!(
                "canonicalize TCMU dir {}: {}",
                tcmu_dir.display(),
                e
            ))
        })?;
        let link_path = lun_dir.join(tcmu_name);
        tokio::fs::symlink(&tcmu_abs, &link_path)
            .await
            .map_err(|e| {
                OverlayBdError::MountFailed(format!(
                    "symlink LUN {} -> {}: {}",
                    link_path.display(),
                    tcmu_abs.display(),
                    e
                ))
            })?;

        // Try to enable the TPG.  For tcm_loop, the LUN symlink alone is enough
        // to trigger SCSI device creation; the TPG enable is optional and may
        // return EPERM on some kernel versions.  Log but don't fail.
        match tokio::fs::write(tpg_dir.join("enable"), "1\n").await {
            Ok(()) => info!("Loopback fabric {} enabled", wwn),
            Err(e) => warn!(
                "Loopback TPG enable for {} returned error (non-fatal): {}",
                wwn, e
            ),
        }

        // Wait for a new sd* device to appear in /sys/block/.
        let dev_name = self.wait_for_new_sd_device(&known).await?;

        // Read major:minor from sysfs and create the block device node in the
        // container's /dev/ (the host's udev won't do this for us here).
        let dev_nums_path = format!("/sys/block/{}/dev", dev_name);
        let dev_nums = tokio::fs::read_to_string(&dev_nums_path)
            .await
            .map_err(|e| OverlayBdError::MountFailed(format!("read {}: {}", dev_nums_path, e)))?;
        let dev_nums = dev_nums.trim();
        let (major_str, minor_str) = dev_nums.split_once(':').ok_or_else(|| {
            OverlayBdError::MountFailed(format!("Unexpected sysfs dev content: {}", dev_nums))
        })?;
        let major: u64 = major_str.parse().map_err(|_| {
            OverlayBdError::MountFailed(format!("Invalid major number: {}", major_str))
        })?;
        let minor: u64 = minor_str.parse().map_err(|_| {
            OverlayBdError::MountFailed(format!("Invalid minor number: {}", minor_str))
        })?;

        let dev_path = format!("/dev/{}", dev_name);
        if !std::path::Path::new(&dev_path).exists() {
            let out = tokio::process::Command::new("mknod")
                .args([&dev_path, "b", &major.to_string(), &minor.to_string()])
                .output()
                .await
                .map_err(|e| OverlayBdError::MountFailed(format!("exec mknod: {}", e)))?;
            if !out.status.success() {
                return Err(OverlayBdError::MountFailed(format!(
                    "mknod {} b {} {} failed: {}",
                    dev_path,
                    major,
                    minor,
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
        }

        Ok(dev_path)
    }

    /// Return the set of "sd*" device names currently present in /sys/block/.
    async fn list_sd_devices(&self) -> Result<HashSet<String>, OverlayBdError> {
        let mut devices = HashSet::new();
        let mut rd = match tokio::fs::read_dir("/sys/block").await {
            Ok(r) => r,
            Err(_) => return Ok(devices),
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            if let Ok(name) = entry.file_name().into_string()
                && name.starts_with("sd")
            {
                devices.insert(name);
            }
        }
        Ok(devices)
    }

    /// Poll /sys/block/ until a new sd* device appears (compared to `known`).
    /// Times out after 10 seconds.
    async fn wait_for_new_sd_device(
        &self,
        known: &HashSet<String>,
    ) -> Result<String, OverlayBdError> {
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let current = self.list_sd_devices().await?;
            for dev in &current {
                if !known.contains(dev) {
                    info!("New SCSI block device detected: {}", dev);
                    return Ok(dev.clone());
                }
            }
        }
        Err(OverlayBdError::MountFailed(
            "No new sd* block device appeared in /sys/block/ after 10s".into(),
        ))
    }

    /// Derive the deterministic TCMU backstore name and loopback WWN from a VM ID.
    ///
    /// Returns `(tcmu_name, wwn)` where:
    /// - `tcmu_name` is `"obd-<12 hex chars from vm_id>"`
    /// - `wwn` is `"naa.<16 hex chars from vm_id>"`
    fn vm_tcmu_names(vm_id: &str) -> (String, String) {
        let hex_id: String = vm_id
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .take(12)
            .collect();
        let tcmu_name = format!("obd-{}", hex_id);
        let wwn_hex: String = vm_id
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .take(16)
            .collect();
        let wwn = format!("naa.{}", wwn_hex);
        (tcmu_name, wwn)
    }
}

/// Build the target image reference in the local registry.
/// e.g. image_ref = "public.ecr.aws/docker/library/ubuntu:22.04",
///      registry_url = "http://registry:5000"
/// ->   "registry:5000/docker/library/ubuntu:22.04"
fn build_target_ref(image_ref: &str, registry_url: &str) -> Result<String, OverlayBdError> {
    let host_owned = registry_host(registry_url);
    let host = host_owned.as_str();

    if host.is_empty() {
        return Err(OverlayBdError::InvalidImageRef(format!(
            "Empty registry host in URL: {}",
            registry_url
        )));
    }

    let bare = if image_ref.contains('/') {
        // If the first component is a registry hostname (contains . or :), strip it.
        // Otherwise, keep the whole string (it's a namespace/repo like "library/ubuntu")
        let (first, rest) = image_ref.split_once('/').unwrap();
        if first.contains('.') || first.contains(':') || first == "localhost" {
            rest
        } else {
            image_ref
        }
    } else {
        image_ref
    };

    Ok(format!("{}/{}", host, bare))
}

/// Parse an image reference into (repo_name, tag).
fn parse_image_ref(image_ref: &str) -> Result<(String, String), OverlayBdError> {
    let without_registry = if image_ref.contains('/') {
        let parts: Vec<&str> = image_ref.splitn(2, '/').collect();
        if parts[0].contains('.') || parts[0].contains(':') {
            parts[1]
        } else {
            image_ref
        }
    } else {
        image_ref
    };

    let (repo, tag) = match without_registry.rsplit_once(':') {
        Some((r, t)) => (r.to_string(), t.to_string()),
        None => (without_registry.to_string(), "latest".to_string()),
    };

    if repo.is_empty() {
        return Err(OverlayBdError::InvalidImageRef(format!(
            "Empty repository in: {}",
            image_ref
        )));
    }

    Ok((repo, tag))
}

fn looks_like_device_path(path: &str) -> bool {
    path.starts_with("/dev/") && path.len() > "/dev/".len()
}

#[cfg(test)]
mod tests {
    use super::looks_like_device_path;

    #[test]
    fn accepts_dev_paths_for_recovery() {
        assert!(looks_like_device_path("/dev/sdb"));
    }

    #[test]
    fn rejects_non_device_recovery_results() {
        assert!(!looks_like_device_path(""));
        assert!(!looks_like_device_path("success"));
        assert!(!looks_like_device_path("sdb"));
    }
}
