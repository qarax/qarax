// gRPC client for communicating with qarax-node

use anyhow::{Context, Result};
use tracing::{debug, instrument};
use uuid::Uuid;

// Include the generated proto code
pub mod node {
    tonic::include_proto!("node");
}

use node::{
    CpusConfig, MemoryConfig, PayloadConfig, VmConfig, VmId, vm_service_client::VmServiceClient,
};

/// Client for communicating with qarax-node via gRPC
pub struct NodeClient {
    address: String,
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
    pub async fn create_vm(
        &self,
        vm_id: Uuid,
        boot_vcpus: i32,
        max_vcpus: i32,
        memory_size: i64,
    ) -> Result<()> {
        debug!("Creating VM {} on node {}", vm_id, self.address);

        let mut client = VmServiceClient::connect(self.address.clone())
            .await
            .context("Failed to connect to qarax-node")?;

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
                kernel: Some("/var/lib/qarax/images/vmlinux".to_string()),
                cmdline: Some("console=ttyS0 reboot=k panic=1 pci=off".to_string()),
                initramfs: None,
                firmware: None,
            }),
            disks: vec![],
            networks: vec![],
            rng: None,
            serial: None,
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
