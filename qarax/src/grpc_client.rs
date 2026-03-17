// gRPC client for communicating with qarax-node

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tracing::{debug, instrument, warn};
use uuid::Uuid;

use crate::model::network_interfaces::{
    NetworkInterface, RateLimiterConfig, TokenBucket, VhostMode,
};
use crate::model::vms::NewVmNetwork;

// Include the generated proto code
pub mod node {
    tonic::include_proto!("node");
}

use node::{
    AddDiskDeviceRequest, AddNetworkDeviceRequest, AttachNetworkRequest, AttachStoragePoolRequest,
    ConsoleConfig, ConsoleInput, ConsoleLogResponse, CopyFileRequest, CpusConfig,
    DetachNetworkRequest, DetachStoragePoolRequest, DiskConfig, DownloadFileRequest, FsConfig,
    ImportOverlayBdRequest, ImportOverlayBdResponse, MemoryConfig, NetConfig, NodeInfo,
    OciImageRequest, OciImageResponse, PayloadConfig, ReceiveMigrationRequest, RemoveDeviceRequest,
    RestoreVmRequest, SendMigrationRequest, SnapshotVmRequest, StoragePoolKind, VmConfig,
    VmCounters, VmId, VmState, file_transfer_service_client::FileTransferServiceClient,
    vm_service_client::VmServiceClient,
};

/// Client for communicating with qarax-node via gRPC
pub struct NodeClient {
    address: String,
}

/// Parameters for creating a VM on the node
#[derive(Debug)]
pub struct CreateVmRequest {
    pub vm_id: Uuid,
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub memory_size: i64,
    pub networks: Vec<NetConfig>,
    pub kernel: Option<String>,
    pub firmware: Option<String>,
    pub initramfs: Option<String>,
    pub cmdline: Option<String>,
    /// Filesystem (virtiofs) devices for OCI image boot
    pub fs_configs: Vec<FsConfig>,
    /// Whether to enable shared memory (required for vhost-user-fs)
    pub memory_shared: bool,
    /// Disk configurations resolved from vm_disks + storage objects
    pub disks: Vec<DiskConfig>,
}

/// Convert DB network interfaces to proto NetConfig for the node.
pub fn net_configs_from_db(networks: &[NetworkInterface]) -> Vec<NetConfig> {
    fn normalize_ip(value: &Option<String>) -> Option<String> {
        value
            .as_deref()
            .map(|v| v.split('/').next().unwrap_or(v).to_string())
    }
    fn normalize_num_queues(value: i32) -> Option<i32> {
        if value <= 1 { None } else { Some(value) }
    }

    networks
        .iter()
        .map(|n| NetConfig {
            id: n.device_id.clone(),
            tap: n.tap_name.clone(),
            ip: normalize_ip(&n.ip_address),
            mask: None,
            mac: n.mac_address.clone(),
            host_mac: n.host_mac.clone(),
            mtu: Some(n.mtu),
            vhost_user: if n.vhost_user { Some(true) } else { None },
            vhost_socket: n.vhost_socket.clone(),
            vhost_mode: n.vhost_mode.as_deref().map(|m| match m {
                "server" => node::VhostMode::Server as i32,
                _ => node::VhostMode::Client as i32,
            }),
            num_queues: normalize_num_queues(n.num_queues),
            queue_size: Some(n.queue_size),
            rate_limiter: None,
            offload_tso: Some(n.offload_tso),
            offload_ufo: Some(n.offload_ufo),
            offload_csum: Some(n.offload_csum),
            pci_segment: if n.pci_segment != 0 {
                Some(n.pci_segment)
            } else {
                None
            },
            iommu: Some(n.iommu),
            bridge: None, // populated later from host_networks lookup
        })
        .collect()
}

/// Convert API network list to proto NetConfig for the node.
pub fn net_configs_from_api(networks: &[NewVmNetwork]) -> Vec<NetConfig> {
    networks
        .iter()
        .map(|n| NetConfig {
            id: n.id.clone(),
            tap: n.tap.clone(),
            ip: n.ip.clone(),
            mask: n.mask.clone(),
            mac: n.mac.clone(),
            host_mac: n.host_mac.clone(),
            mtu: n.mtu,
            vhost_user: n.vhost_user,
            vhost_socket: n.vhost_socket.clone(),
            vhost_mode: n.vhost_mode.as_ref().map(|m| match m {
                VhostMode::Server => node::VhostMode::Server as i32,
                VhostMode::Client => node::VhostMode::Client as i32,
            }),
            num_queues: n.num_queues,
            queue_size: n.queue_size,
            rate_limiter: n.rate_limiter.as_ref().map(rate_limiter_to_proto),
            offload_tso: n.offload_tso,
            offload_ufo: n.offload_ufo,
            offload_csum: n.offload_csum,
            pci_segment: n.pci_segment,
            iommu: n.iommu,
            bridge: None,
        })
        .collect()
}

fn rate_limiter_to_proto(r: &RateLimiterConfig) -> node::RateLimiterConfig {
    node::RateLimiterConfig {
        bandwidth: r.bandwidth.as_ref().map(token_bucket_to_proto),
        ops: r.ops.as_ref().map(token_bucket_to_proto),
    }
}

fn token_bucket_to_proto(b: &TokenBucket) -> node::TokenBucket {
    node::TokenBucket {
        size: b.size,
        refill_time: b.refill_time,
        one_time_burst: b.one_time_burst,
    }
}

impl NodeClient {
    /// Create a new client for the specified qarax-node address
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            address: format!("http://{}:{}", host, port),
        }
    }

    /// Create a new client from a full address string (host:port)
    pub fn from_address(address: &str) -> Self {
        Self {
            address: format!("http://{}", address),
        }
    }

    async fn connect_vm_service(&self) -> Result<VmServiceClient<tonic::transport::Channel>> {
        VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")
    }

    async fn connect_vm_service_with_retry(
        &self,
        attempts: usize,
        delay: Duration,
    ) -> Result<VmServiceClient<tonic::transport::Channel>> {
        let mut last_error = None;

        for attempt in 1..=attempts {
            match self.connect_vm_service().await {
                Ok(client) => return Ok(client),
                Err(error) => {
                    let is_refused = error
                        .chain()
                        .any(|cause| cause.to_string().contains("Connection refused"));
                    if !is_refused || attempt == attempts {
                        return Err(error);
                    }

                    warn!(
                        address = %self.address,
                        attempt,
                        attempts,
                        error = %error,
                        "qarax-node connection refused, retrying"
                    );
                    last_error = Some(error);
                    sleep(delay).await;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Failed to connect to qarax-node")))
    }

    /// Create a VM on the qarax-node
    #[instrument(skip(self))]
    pub async fn create_vm(&self, req: CreateVmRequest) -> Result<()> {
        let CreateVmRequest {
            vm_id,
            boot_vcpus,
            max_vcpus,
            memory_size,
            networks,
            kernel,
            firmware,
            initramfs,
            cmdline,
            fs_configs,
            memory_shared,
            disks: extra_disks,
        } = req;
        debug!("Creating VM {} on node {}", vm_id, self.address);

        let mut client = self.connect_vm_service().await?;

        // Check if a production rootfs is configured via environment variable
        let mut disks = vec![];
        if let Ok(rootfs_path) = std::env::var("VM_ROOTFS")
            && !rootfs_path.is_empty()
        {
            debug!("Adding rootfs disk: {}", rootfs_path);
            disks.push(DiskConfig {
                id: "rootfs".to_string(),
                path: Some(rootfs_path),
                readonly: Some(false),
                direct: None,
                vhost_user: None,
                vhost_socket: None,
                num_queues: None,
                queue_size: None,
                rate_limiter: None,
                rate_limit_group: None,
                pci_segment: None,
                serial: None,
                oci_image_ref: None,
                registry_url: None,
            });
        }

        // Append disks resolved from vm_disks + storage objects
        disks.extend(extra_disks);

        let config = VmConfig {
            vm_id: vm_id.to_string(),
            cpus: Some(CpusConfig {
                boot_vcpus,
                max_vcpus,
                topology: None,
                kvm_hyperv: None,
                max_phys_bits: None,
            }),
            memory: Some(MemoryConfig {
                size: memory_size,
                hotplug_size: None,
                mergeable: None,
                shared: if memory_shared { Some(true) } else { None },
                hugepages: None,
                hugepage_size: None,
                prefault: None,
                thp: None,
            }),
            payload: Some(PayloadConfig {
                kernel: kernel.filter(|s| !s.is_empty()),
                cmdline: cmdline.filter(|s| !s.is_empty()),
                initramfs: initramfs.filter(|s| !s.trim().is_empty()),
                firmware: firmware.filter(|s| !s.is_empty()),
            }),
            disks,
            networks,
            rng: None,
            // Serial console in PTY mode for interactive access
            serial: Some(ConsoleConfig {
                mode: 1, // CONSOLE_MODE_PTY
                file: None,
                socket: None,
                iommu: None,
            }),
            console: None,
            rate_limit_groups: vec![],
            fs: fs_configs,
        };

        client
            .create_vm(config)
            .await
            .context("Failed to create VM on qarax-node")?;

        debug!("VM {} created successfully", vm_id);
        Ok(())
    }

    /// Start a VM on the qarax-node
    #[instrument(skip(self))]
    pub async fn start_vm(&self, vm_id: Uuid) -> Result<()> {
        debug!("Starting VM {} on node {}", vm_id, self.address);

        let mut client = self
            .connect_vm_service_with_retry(8, Duration::from_secs(1))
            .await?;

        client
            .start_vm(VmId {
                id: vm_id.to_string(),
            })
            .await
            .context("Failed to start VM on qarax-node")?;

        debug!("VM {} started successfully", vm_id);
        Ok(())
    }

    /// Stop a VM on the qarax-node
    #[instrument(skip(self))]
    pub async fn stop_vm(&self, vm_id: Uuid) -> Result<()> {
        debug!("Stopping VM {} on node {}", vm_id, self.address);

        let mut client = match VmServiceClient::connect(self.address.clone()).await {
            Ok(c) => c,
            Err(_) => {
                // Node is unreachable — treat as already stopped
                return Err(crate::errors::Error::NotFound.into());
            }
        };

        client
            .stop_vm(VmId {
                id: vm_id.to_string(),
            })
            .await
            .map_err(|s| match s.code() {
                // VM or CH process gone — treat as already stopped
                tonic::Code::NotFound
                | tonic::Code::Unknown
                | tonic::Code::Unavailable
                | tonic::Code::Internal => crate::errors::Error::NotFound.into(),
                _ => anyhow::anyhow!("Failed to stop VM on qarax-node: {}", s),
            })?;

        debug!("VM {} stopped successfully", vm_id);
        Ok(())
    }

    /// Pause a VM on the qarax-node
    #[instrument(skip(self))]
    pub async fn pause_vm(&self, vm_id: Uuid) -> Result<()> {
        debug!("Pausing VM {} on node {}", vm_id, self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        client
            .pause_vm(VmId {
                id: vm_id.to_string(),
            })
            .await
            .context("Failed to pause VM on qarax-node")?;

        debug!("VM {} paused successfully", vm_id);
        Ok(())
    }

    /// Resume a VM on the qarax-node
    #[instrument(skip(self))]
    pub async fn resume_vm(&self, vm_id: Uuid) -> Result<()> {
        debug!("Resuming VM {} on node {}", vm_id, self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        client
            .resume_vm(VmId {
                id: vm_id.to_string(),
            })
            .await
            .context("Failed to resume VM on qarax-node")?;

        debug!("VM {} resumed successfully", vm_id);
        Ok(())
    }

    /// Snapshot a VM on the qarax-node
    #[instrument(skip(self))]
    pub async fn snapshot_vm(&self, vm_id: Uuid, snapshot_url: &str) -> Result<()> {
        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;
        client
            .snapshot_vm(SnapshotVmRequest {
                vm_id: vm_id.to_string(),
                snapshot_url: snapshot_url.to_string(),
            })
            .await
            .context("Failed to snapshot VM on qarax-node")?;
        Ok(())
    }

    /// Restore a VM on the qarax-node from a snapshot
    #[instrument(skip(self))]
    pub async fn restore_vm(&self, vm_id: Uuid, source_url: &str) -> Result<()> {
        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;
        client
            .restore_vm(RestoreVmRequest {
                vm_id: vm_id.to_string(),
                source_url: source_url.to_string(),
            })
            .await
            .context("Failed to restore VM on qarax-node")?;
        Ok(())
    }

    /// Get live VM info from the qarax-node
    #[instrument(skip(self))]
    pub async fn get_vm_info(&self, vm_id: Uuid) -> Result<VmState> {
        debug!("Getting VM info {} from node {}", vm_id, self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        let response = client
            .get_vm_info(VmId {
                id: vm_id.to_string(),
            })
            .await
            .context("Failed to get VM info from qarax-node")?;

        Ok(response.into_inner())
    }

    /// Get live VM counters from the qarax-node
    #[instrument(skip(self))]
    pub async fn get_vm_counters(&self, vm_id: Uuid) -> Result<VmCounters> {
        debug!("Getting VM counters {} from node {}", vm_id, self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        let response = client
            .get_vm_counters(VmId {
                id: vm_id.to_string(),
            })
            .await
            .context("Failed to get VM counters from qarax-node")?;

        Ok(response.into_inner())
    }

    /// Download a file on the node from a URL to a destination path
    #[instrument(skip(self))]
    pub async fn download_file(
        &self,
        transfer_id: &str,
        source_url: &str,
        destination_path: &str,
    ) -> Result<i64> {
        debug!(
            "Requesting file download on node {}: {} -> {}",
            self.address, source_url, destination_path
        );

        let mut client = FileTransferServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        let response = client
            .download_file(DownloadFileRequest {
                transfer_id: transfer_id.to_string(),
                source_url: source_url.to_string(),
                destination_path: destination_path.to_string(),
            })
            .await
            .map_err(|s| {
                anyhow::anyhow!(
                    "gRPC download_file failed: code={:?} message={}",
                    s.code(),
                    s.message()
                )
            })?
            .into_inner();

        if response.success {
            Ok(response.bytes_written)
        } else {
            anyhow::bail!("Download failed: {}", response.error)
        }
    }

    /// Copy a file locally on the node from source to destination
    #[instrument(skip(self))]
    pub async fn copy_file(
        &self,
        transfer_id: &str,
        source_path: &str,
        destination_path: &str,
    ) -> Result<i64> {
        debug!(
            "Requesting file copy on node {}: {} -> {}",
            self.address, source_path, destination_path
        );

        let mut client = FileTransferServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        let response = client
            .copy_file(CopyFileRequest {
                transfer_id: transfer_id.to_string(),
                source_path: source_path.to_string(),
                destination_path: destination_path.to_string(),
            })
            .await
            .map_err(|s| {
                anyhow::anyhow!(
                    "gRPC copy_file failed: code={:?} message={}",
                    s.code(),
                    s.message()
                )
            })?
            .into_inner();

        if response.success {
            Ok(response.bytes_written)
        } else {
            anyhow::bail!("Copy failed: {}", response.error)
        }
    }

    /// Import (convert + push) an OCI image for OverlayBD lazy loading
    #[instrument(skip(self))]
    pub async fn import_overlaybd_image(
        &self,
        image_ref: &str,
        registry_url: &str,
    ) -> Result<ImportOverlayBdResponse> {
        debug!(
            "Importing OverlayBD image {} to {} on node {}",
            image_ref, registry_url, self.address
        );

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        let response = client
            .import_overlay_bd_image(ImportOverlayBdRequest {
                image_ref: image_ref.to_string(),
                registry_url: registry_url.to_string(),
            })
            .await
            .map_err(|s| {
                anyhow::anyhow!(
                    "gRPC import_overlaybd_image failed: code={:?} message={}",
                    s.code(),
                    s.message()
                )
            })?;

        Ok(response.into_inner())
    }

    /// Pull an OCI image on the qarax-node via Nydus
    #[instrument(skip(self))]
    pub async fn pull_image(&self, image_ref: &str) -> Result<OciImageResponse> {
        debug!("Pulling image {} on node {}", image_ref, self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        let response = client
            .pull_image(OciImageRequest {
                image_ref: image_ref.to_string(),
            })
            .await
            .map_err(|s| anyhow::anyhow!("Failed to pull image on qarax-node: {}", s.message()))?;

        Ok(response.into_inner())
    }

    /// Delete a VM on the qarax-node
    #[instrument(skip(self))]
    pub async fn delete_vm(&self, vm_id: Uuid) -> Result<()> {
        debug!("Deleting VM {} on node {}", vm_id, self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        client
            .delete_vm(VmId {
                id: vm_id.to_string(),
            })
            .await
            .context("Failed to delete VM on qarax-node")?;

        debug!("VM {} deleted successfully", vm_id);
        Ok(())
    }

    /// Attach a storage pool on the node (mount NFS, verify OverlayBD registry, create local dir).
    /// Returns an error if the pool kind is unrecognised or the underlying operation fails.
    #[instrument(skip(self))]
    pub async fn attach_storage_pool(
        &self,
        pool: &crate::model::storage_pools::StoragePool,
    ) -> Result<()> {
        use crate::model::storage_pools::StoragePoolType;

        debug!(
            "Attaching storage pool {} ({}) on node {}",
            pool.id, pool.pool_type, self.address
        );

        let pool_kind = match pool.pool_type {
            StoragePoolType::Local => StoragePoolKind::Local,
            StoragePoolType::Nfs => StoragePoolKind::Nfs,
            StoragePoolType::OverlayBd => StoragePoolKind::Overlaybd,
        };

        let config_json = pool.config.to_string();

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        let response = client
            .attach_storage_pool(AttachStoragePoolRequest {
                pool_id: pool.id.to_string(),
                pool_kind: pool_kind as i32,
                config_json,
            })
            .await
            .map_err(|s| {
                anyhow::anyhow!(
                    "gRPC attach_storage_pool failed: code={:?} message={}",
                    s.code(),
                    s.message()
                )
            })?
            .into_inner();

        if response.success {
            debug!("Storage pool {} attached: {}", pool.id, response.message);
            Ok(())
        } else {
            anyhow::bail!("attach_storage_pool failed: {}", response.message)
        }
    }

    /// Detach a storage pool from the node (unmount NFS, no-op for others).
    #[instrument(skip(self))]
    pub async fn detach_storage_pool(
        &self,
        pool: &crate::model::storage_pools::StoragePool,
    ) -> Result<()> {
        use crate::model::storage_pools::StoragePoolType;

        debug!(
            "Detaching storage pool {} ({}) on node {}",
            pool.id, pool.pool_type, self.address
        );

        let pool_kind = match pool.pool_type {
            StoragePoolType::Local => StoragePoolKind::Local,
            StoragePoolType::Nfs => StoragePoolKind::Nfs,
            StoragePoolType::OverlayBd => StoragePoolKind::Overlaybd,
        };

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        client
            .detach_storage_pool(DetachStoragePoolRequest {
                pool_id: pool.id.to_string(),
                pool_kind: pool_kind as i32,
            })
            .await
            .map_err(|s| {
                anyhow::anyhow!(
                    "gRPC detach_storage_pool failed: code={:?} message={}",
                    s.code(),
                    s.message()
                )
            })?;

        debug!("Storage pool {} detached", pool.id);
        Ok(())
    }

    /// Attach a network (create bridge, start DHCP server, setup NAT) on the node.
    /// If `parent_interface` is non-empty, bridges that NIC instead of creating
    /// an isolated bridge (skips NAT).
    #[instrument(skip(self))]
    #[allow(clippy::too_many_arguments)]
    pub async fn attach_network(
        &self,
        bridge_name: &str,
        subnet: &str,
        gateway: &str,
        dns: &str,
        dhcp_range_start: &str,
        dhcp_range_end: &str,
        parent_interface: &str,
    ) -> Result<()> {
        debug!(
            "Attaching network bridge {} on node {}",
            bridge_name, self.address
        );

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        client
            .attach_network(AttachNetworkRequest {
                bridge_name: bridge_name.to_string(),
                subnet: subnet.to_string(),
                gateway: gateway.to_string(),
                dns: dns.to_string(),
                dhcp_range_start: dhcp_range_start.to_string(),
                dhcp_range_end: dhcp_range_end.to_string(),
                parent_interface: parent_interface.to_string(),
            })
            .await
            .map_err(|s| {
                anyhow::anyhow!(
                    "gRPC attach_network failed: code={:?} message={}",
                    s.code(),
                    s.message()
                )
            })?;

        debug!("Network bridge {} attached", bridge_name);
        Ok(())
    }

    /// Detach a network (stop DHCP server, teardown NAT, delete bridge) on the node.
    #[instrument(skip(self))]
    pub async fn detach_network(&self, bridge_name: &str) -> Result<()> {
        debug!(
            "Detaching network bridge {} on node {}",
            bridge_name, self.address
        );

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        client
            .detach_network(DetachNetworkRequest {
                bridge_name: bridge_name.to_string(),
            })
            .await
            .map_err(|s| {
                anyhow::anyhow!(
                    "gRPC detach_network failed: code={:?} message={}",
                    s.code(),
                    s.message()
                )
            })?;

        debug!("Network bridge {} detached", bridge_name);
        Ok(())
    }

    /// Read the console log for a VM on the qarax-node
    #[instrument(skip(self))]
    pub async fn read_console_log(&self, vm_id: Uuid) -> Result<ConsoleLogResponse> {
        debug!(
            "Reading console log for VM {} from node {}",
            vm_id, self.address
        );

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        let response = client
            .read_console_log(VmId {
                id: vm_id.to_string(),
            })
            .await
            .context("Failed to read console log from qarax-node")?;

        Ok(response.into_inner())
    }

    /// Get node information (versions, hostname) from the qarax-node
    #[instrument(skip(self))]
    pub async fn get_node_info(&self) -> Result<NodeInfo> {
        debug!("Getting node info from {}", self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        let response = client
            .get_node_info(())
            .await
            .context("Failed to get node info from qarax-node")?;

        Ok(response.into_inner())
    }

    /// Attach to VM console for interactive bidirectional I/O
    /// Returns (input_sender, output_receiver) channels for WebSocket proxying
    #[instrument(skip(self))]
    pub async fn attach_console(&self, vm_id: Uuid) -> Result<ConsoleChannel> {
        debug!("Attaching to console for VM {} via {}", vm_id, self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        // Create channels for bidirectional communication
        let (input_tx, mut input_rx) = mpsc::channel::<Vec<u8>>(128);
        let (output_tx, output_rx) = mpsc::channel::<Result<Vec<u8>>>(128);

        // Create gRPC input stream
        let input_stream = async_stream::stream! {
            // Send initial message with VM ID
            yield ConsoleInput {
                vm_id: vm_id.to_string(),
                data: vec![],
                resize: false,
                rows: 0,
                cols: 0,
            };

            // Forward data from input channel to gRPC
            while let Some(data) = input_rx.recv().await {
                yield ConsoleInput {
                    vm_id: String::new(), // Only needed in first message
                    data,
                    resize: false,
                    rows: 0,
                    cols: 0,
                };
            }
        };

        // Call the streaming RPC
        let response = client
            .attach_console(input_stream)
            .await
            .context("Failed to attach to console")?;

        let mut output_stream = response.into_inner();

        // Spawn task to forward gRPC output to our channel
        tokio::spawn(async move {
            while let Ok(Some(msg)) = output_stream.message().await {
                let result = if msg.error {
                    Err(anyhow::anyhow!(msg.error_message))
                } else {
                    Ok(msg.data)
                };

                if output_tx.send(result).await.is_err() {
                    break;
                }
            }
        });

        Ok((input_tx, output_rx))
    }

    /// Hotplug a disk device into a running VM
    #[instrument(skip(self))]
    pub async fn add_disk_device(&self, vm_id: Uuid, config: DiskConfig) -> Result<()> {
        debug!(
            "Hotplugging disk {} to VM {} on node {}",
            config.id, vm_id, self.address
        );
        let mut client = self.connect_vm_service().await?;
        client
            .add_disk_device(AddDiskDeviceRequest {
                vm_id: vm_id.to_string(),
                config: Some(config),
            })
            .await
            .context("Failed to hotplug disk device on qarax-node")?;
        Ok(())
    }

    /// Hotunplug a disk device from a running VM
    #[instrument(skip(self))]
    pub async fn remove_disk_device(&self, vm_id: Uuid, device_id: &str) -> Result<()> {
        debug!(
            "Hotunplugging disk {} from VM {} on node {}",
            device_id, vm_id, self.address
        );
        let mut client = self.connect_vm_service().await?;
        client
            .remove_disk_device(RemoveDeviceRequest {
                vm_id: vm_id.to_string(),
                device_id: device_id.to_string(),
            })
            .await
            .context("Failed to hotunplug disk device on qarax-node")?;
        Ok(())
    }

    /// Hotplug a network device into a running VM
    #[instrument(skip(self))]
    pub async fn add_network_device(&self, vm_id: Uuid, config: NetConfig) -> Result<()> {
        debug!(
            "Hotplugging NIC {} to VM {} on node {}",
            config.id, vm_id, self.address
        );
        let mut client = self.connect_vm_service().await?;
        client
            .add_network_device(AddNetworkDeviceRequest {
                vm_id: vm_id.to_string(),
                config: Some(config),
            })
            .await
            .context("Failed to hotplug network device on qarax-node")?;
        Ok(())
    }

    /// Hotunplug a network device from a running VM
    #[instrument(skip(self))]
    pub async fn remove_network_device(&self, vm_id: Uuid, device_id: &str) -> Result<()> {
        debug!(
            "Hotunplugging NIC {} from VM {} on node {}",
            device_id, vm_id, self.address
        );
        let mut client = self.connect_vm_service().await?;
        client
            .remove_network_device(RemoveDeviceRequest {
                vm_id: vm_id.to_string(),
                device_id: device_id.to_string(),
            })
            .await
            .context("Failed to hotunplug network device on qarax-node")?;
        Ok(())
    }

    /// Prepare the destination node to receive a live migration.
    ///
    /// Returns the `receiver_url` that Cloud Hypervisor is listening on
    /// (e.g. `"tcp://0.0.0.0:49152"`).  Callers must replace `0.0.0.0` with
    /// the destination host's real IP before passing it to `send_migration`.
    #[instrument(skip(self, config))]
    pub async fn receive_migration(
        &self,
        vm_id: Uuid,
        config: VmConfig,
        migration_port: u16,
    ) -> Result<String> {
        let mut client = self
            .connect_vm_service()
            .await
            .context("Failed to connect to destination qarax-node")?;

        let response = client
            .receive_migration(ReceiveMigrationRequest {
                vm_id: vm_id.to_string(),
                config: Some(config),
                migration_port: migration_port as i32,
            })
            .await
            .context("receive_migration RPC failed")?;

        Ok(response.into_inner().receiver_url)
    }

    /// Initiate an outbound live migration on the source node.
    ///
    /// `destination_url` must point to the real IP and port returned by
    /// `receive_migration` (e.g. `"tcp://192.168.1.20:49152"`).
    ///
    /// This call blocks until Cloud Hypervisor finishes transferring all dirty
    /// pages and the VM is running on the destination.
    #[instrument(skip(self))]
    pub async fn send_migration(&self, vm_id: Uuid, destination_url: &str) -> Result<()> {
        let mut client = self
            .connect_vm_service()
            .await
            .context("Failed to connect to source qarax-node")?;

        client
            .send_migration(SendMigrationRequest {
                vm_id: vm_id.to_string(),
                destination_url: destination_url.to_string(),
            })
            .await
            .context("send_migration RPC failed")?;

        Ok(())
    }
}

/// Type alias for console channel used in attach_console
pub type ConsoleChannel = (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Result<Vec<u8>>>);
