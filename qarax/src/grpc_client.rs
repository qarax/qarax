// gRPC client for communicating with qarax-node

use anyhow::{Context, Result};
use tracing::{debug, instrument};
use uuid::Uuid;

use crate::model::vms::NewVmNetwork;

// Include the generated proto code
pub mod node {
    tonic::include_proto!("node");
}

use node::{
    ConsoleConfig, CpusConfig, DiskConfig, MemoryConfig, NetConfig, PayloadConfig, VmConfig, VmId,
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
    pub kernel: String,
    pub initramfs: Option<String>,
    pub cmdline: String,
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
            host_mac: None,
            mtu: n.mtu,
            vhost_user: None,
            vhost_socket: None,
            vhost_mode: None,
            num_queues: None,
            queue_size: None,
            rate_limiter: None,
            offload_tso: None,
            offload_ufo: None,
            offload_csum: None,
            pci_segment: None,
            iommu: None,
        })
        .collect()
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
            initramfs,
            cmdline,
        } = req;
        debug!("Creating VM {} on node {}", vm_id, self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

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
            });
        }

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
                shared: None,
                hugepages: None,
                hugepage_size: None,
                prefault: None,
                thp: None,
            }),
            payload: Some(PayloadConfig {
                kernel: Some(kernel),
                cmdline: Some(cmdline),
                initramfs: initramfs.filter(|s| !s.is_empty()), // Skip empty initramfs
                firmware: None,
            }),
            disks,
            networks,
            rng: None,
            // Serial console to file so VM output can be viewed: /var/lib/qarax/vms/{vm_id}.console.log
            serial: Some(ConsoleConfig {
                mode: 3, // CONSOLE_MODE_FILE
                file: Some(format!("/var/lib/qarax/vms/{}.console.log", vm_id)),
                socket: None,
                iommu: None,
            }),
            console: None,
            rate_limit_groups: vec![],
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

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

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

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

        client
            .stop_vm(VmId {
                id: vm_id.to_string(),
            })
            .await
            .context("Failed to stop VM on qarax-node")?;

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
}
