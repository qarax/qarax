use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction, Type, types::Json};
use strum_macros::{Display, EnumString};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    errors::Error,
    model::{instance_types, vm_templates},
};

use crate::model::network_interfaces::{InterfaceType, RateLimiterConfig, VhostMode};

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct CpuTopology {
    pub threads_per_core: Option<i32>,
    pub cores_per_die: Option<i32>,
    pub dies_per_package: Option<i32>,
    pub packages: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Vm {
    pub id: Uuid,
    pub name: String,
    pub tags: Vec<String>,
    pub host_id: Option<Uuid>,
    pub status: VmStatus,
    pub hypervisor: Hypervisor,
    pub boot_source_id: Option<Uuid>,
    pub boot_mode: BootMode,
    pub description: Option<String>,

    // CPU configuration
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub cpu_topology: Option<serde_json::Value>,
    pub kvm_hyperv: bool,

    // Memory configuration (in bytes)
    pub memory_size: i64,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: bool,
    pub memory_shared: bool,
    pub memory_hugepages: bool,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: bool,
    pub memory_thp: bool,

    // OCI image boot support
    pub image_ref: Option<String>,

    // Cloud-init NoCloud seed data
    pub cloud_init_user_data: Option<String>,
    pub cloud_init_meta_data: Option<String>,
    pub cloud_init_network_config: Option<String>,

    // Legacy config field for flexibility
    pub config: serde_json::Value,
}

#[derive(sqlx::FromRow)]
pub struct VmRow {
    pub id: Uuid,
    pub name: String,
    pub tags: Vec<String>,
    pub host_id: Option<Uuid>,
    pub status: VmStatus,
    pub hypervisor: Hypervisor,
    pub boot_source_id: Option<Uuid>,
    pub boot_mode: BootMode,
    pub description: Option<String>,

    // CPU configuration
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub cpu_topology: Option<Json<serde_json::Value>>,
    pub kvm_hyperv: bool,

    // Memory configuration
    pub memory_size: i64,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: bool,
    pub memory_shared: bool,
    pub memory_hugepages: bool,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: bool,
    pub memory_thp: bool,

    pub image_ref: Option<String>,

    pub cloud_init_user_data: Option<String>,
    pub cloud_init_meta_data: Option<String>,
    pub cloud_init_network_config: Option<String>,

    pub config: Json<serde_json::Value>,
}

impl From<VmRow> for Vm {
    fn from(row: VmRow) -> Self {
        Vm {
            id: row.id,
            name: row.name,
            tags: row.tags,
            status: row.status,
            host_id: row.host_id,
            hypervisor: row.hypervisor,
            boot_source_id: row.boot_source_id,
            boot_mode: row.boot_mode,
            description: row.description,

            boot_vcpus: row.boot_vcpus,
            max_vcpus: row.max_vcpus,
            cpu_topology: row.cpu_topology.map(|t| t.0),
            kvm_hyperv: row.kvm_hyperv,

            memory_size: row.memory_size,
            memory_hotplug_size: row.memory_hotplug_size,
            memory_mergeable: row.memory_mergeable,
            memory_shared: row.memory_shared,
            memory_hugepages: row.memory_hugepages,
            memory_hugepage_size: row.memory_hugepage_size,
            memory_prefault: row.memory_prefault,
            memory_thp: row.memory_thp,

            image_ref: row.image_ref,

            cloud_init_user_data: row.cloud_init_user_data,
            cloud_init_meta_data: row.cloud_init_meta_data,
            cloud_init_network_config: row.cloud_init_network_config,

            config: row.config.0,
        }
    }
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "hypervisor")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Hypervisor {
    CloudHv,
    Firecracker,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "boot_mode")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum BootMode {
    Kernel,
    Firmware,
}

#[derive(
    Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display, ToSchema,
)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "vm_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VmStatus {
    Unknown,
    Pending,
    Created,
    Running,
    Paused,
    Shutdown,
    Migrating,
    Committing,
}

/// Network interface config for create-VM request. Passed to qarax-node; id is required.
#[derive(Serialize, Deserialize, Debug, Clone, Default, ToSchema)]
pub struct NewVmNetwork {
    /// Unique device id (e.g. "net0")
    pub id: String,
    /// Network ID for managed networking (IPAM)
    #[serde(default)]
    pub network_id: Option<Uuid>,
    /// Guest MAC address (optional)
    pub mac: Option<String>,
    /// Pre-created TAP device name (optional)
    pub tap: Option<String>,
    /// IPv4 or IPv6 address (optional)
    pub ip: Option<String>,
    /// Network mask (optional)
    pub mask: Option<String>,
    /// MTU (optional)
    pub mtu: Option<i32>,

    // Advanced fields
    /// Host-side MAC address
    pub host_mac: Option<String>,
    /// Override interface type (inferred from tap/vhost_user if not set)
    pub interface_type: Option<InterfaceType>,
    /// Enable vhost-user networking
    pub vhost_user: Option<bool>,
    /// Unix socket path for vhost-user backend
    pub vhost_socket: Option<String>,
    /// Vhost-user mode (client or server)
    pub vhost_mode: Option<VhostMode>,
    /// Number of virtio queues
    pub num_queues: Option<i32>,
    /// Size of each queue
    pub queue_size: Option<i32>,
    /// Rate limiter configuration
    pub rate_limiter: Option<RateLimiterConfig>,
    /// Enable TCP Segmentation Offload
    pub offload_tso: Option<bool>,
    /// Enable UDP Fragmentation Offload
    pub offload_ufo: Option<bool>,
    /// Enable checksum offload
    pub offload_csum: Option<bool>,
    /// PCI segment number
    pub pci_segment: Option<i32>,
    /// Enable IOMMU for the device
    pub iommu: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct NewVm {
    pub name: String,
    pub tags: Option<Vec<String>>,
    pub vm_template_id: Option<Uuid>,
    pub instance_type_id: Option<Uuid>,
    pub hypervisor: Option<Hypervisor>,
    pub architecture: Option<String>,

    // CPU
    pub boot_vcpus: Option<i32>,
    pub max_vcpus: Option<i32>,
    pub cpu_topology: Option<serde_json::Value>,
    pub kvm_hyperv: Option<bool>,

    // Memory
    pub memory_size: Option<i64>,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: Option<bool>,
    pub memory_shared: Option<bool>,
    pub memory_hugepages: Option<bool>,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: Option<bool>,
    pub memory_thp: Option<bool>,

    pub boot_source_id: Option<Uuid>,
    pub root_disk_object_id: Option<Uuid>,
    pub boot_mode: Option<BootMode>,
    pub description: Option<String>,

    /// OCI image reference to use as root filesystem (e.g. "docker.io/library/ubuntu:22.04").
    /// When set, the handler will check whether the selected host has an OverlayBD storage pool
    /// and the image is served via lazy block loading (virtio-blk).
    pub image_ref: Option<String>,

    /// Cloud-init user-data (raw YAML). When provided a NoCloud seed image is
    /// generated and attached as a read-only disk to the VM.
    pub cloud_init_user_data: Option<String>,
    /// Cloud-init meta-data (raw YAML). Auto-generated from vm id/name if omitted.
    pub cloud_init_meta_data: Option<String>,
    /// Cloud-init network-config (raw YAML). When provided, kernel `ip=` cmdline
    /// params are suppressed so cloud-init owns networking.
    pub cloud_init_network_config: Option<String>,

    /// Network ID to attach the VM to (triggers IPAM allocation).
    pub network_id: Option<Uuid>,

    /// Optional network interfaces to attach at create time (passed to qarax-node).
    #[serde(default)]
    pub networks: Option<Vec<NewVmNetwork>>,

    /// Security groups to bind to the VM. Rules apply to managed routed traffic
    /// on every managed NIC attached to the VM.
    #[serde(default)]
    #[schema(value_type = Option<Vec<String>>)]
    pub security_group_ids: Option<Vec<Uuid>>,

    /// Accelerator (GPU) configuration. When set, GPU-aware scheduling picks a
    /// host with available GPUs matching these filters, and VFIO passthrough
    /// devices are attached to the VM.
    pub accelerator_config: Option<serde_json::Value>,

    /// NUMA configuration. When set, the VM is pinned to the specified NUMA node.
    /// If accelerator_config has prefer_local_numa=true (the default), GPU-local NUMA
    /// is used instead and this field is ignored.
    pub numa_config: Option<serde_json::Value>,

    /// When set alongside `image_ref`, the OverlayBD upper layer (upper.data +
    /// upper.index) is stored as a persistent `OverlaybdUpper` StorageObject on
    /// this pool instead of being ephemeral. The pool must be Local or NFS and
    /// must be attached to the host running the VM.
    pub persistent_upper_pool_id: Option<Uuid>,

    #[serde(default = "default_vm_config")]
    pub config: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ResolvedNewVm {
    pub name: String,
    pub tags: Vec<String>,
    pub hypervisor: Hypervisor,
    pub architecture: Option<String>,
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub cpu_topology: Option<serde_json::Value>,
    pub kvm_hyperv: Option<bool>,
    pub memory_size: i64,
    pub memory_hotplug_size: Option<i64>,
    pub memory_mergeable: Option<bool>,
    pub memory_shared: Option<bool>,
    pub memory_hugepages: Option<bool>,
    pub memory_hugepage_size: Option<i64>,
    pub memory_prefault: Option<bool>,
    pub memory_thp: Option<bool>,
    pub boot_source_id: Option<Uuid>,
    pub root_disk_object_id: Option<Uuid>,
    pub boot_mode: Option<BootMode>,
    pub description: Option<String>,
    pub image_ref: Option<String>,
    pub cloud_init_user_data: Option<String>,
    pub cloud_init_meta_data: Option<String>,
    pub cloud_init_network_config: Option<String>,
    pub network_id: Option<Uuid>,
    pub networks: Option<Vec<NewVmNetwork>>,
    pub security_group_ids: Option<Vec<Uuid>>,
    pub accelerator_config: Option<serde_json::Value>,
    pub numa_config: Option<serde_json::Value>,
    pub persistent_upper_pool_id: Option<Uuid>,
    pub config: serde_json::Value,
}

fn default_vm_config() -> serde_json::Value {
    serde_json::json!({})
}

fn merge_config(
    template_config: serde_json::Value,
    request_config: serde_json::Value,
) -> serde_json::Value {
    match (template_config, request_config) {
        (serde_json::Value::Object(mut template), serde_json::Value::Object(request)) => {
            template.extend(request);
            serde_json::Value::Object(template)
        }
        (serde_json::Value::Null, serde_json::Value::Null) => default_vm_config(),
        (_, serde_json::Value::Null) => default_vm_config(),
        (serde_json::Value::Null, request) => request,
        (_, request) => request,
    }
}

pub async fn resolve_create_request(pool: &PgPool, request: NewVm) -> Result<ResolvedNewVm, Error> {
    let NewVm {
        name,
        tags,
        vm_template_id,
        instance_type_id,
        hypervisor,
        architecture,
        boot_vcpus,
        max_vcpus,
        cpu_topology,
        kvm_hyperv,
        memory_size,
        memory_hotplug_size,
        memory_mergeable,
        memory_shared,
        memory_hugepages,
        memory_hugepage_size,
        memory_prefault,
        memory_thp,
        boot_source_id,
        root_disk_object_id,
        boot_mode,
        description,
        image_ref,
        cloud_init_user_data,
        cloud_init_meta_data,
        cloud_init_network_config,
        network_id,
        networks,
        security_group_ids,
        accelerator_config,
        numa_config,
        persistent_upper_pool_id,
        config,
    } = request;

    let vm_template = match vm_template_id {
        Some(id) => Some(vm_templates::get(pool, id).await?),
        None => None,
    };
    let instance_type = match instance_type_id {
        Some(id) => Some(instance_types::get(pool, id).await?),
        None => None,
    };

    let architecture = architecture
        .or(instance_type
            .as_ref()
            .and_then(|it| it.architecture.clone()))
        .and_then(|arch| common::architecture::normalize_architecture(&arch));

    let boot_vcpus = boot_vcpus
        .or(instance_type.as_ref().map(|it| it.boot_vcpus))
        .or(vm_template.as_ref().and_then(|template| template.boot_vcpus))
        .ok_or_else(|| {
            Error::UnprocessableEntity(
                "boot_vcpus is required unless provided by the selected instance type or VM template"
                    .into(),
            )
        })?;

    let direct_max_vcpus = max_vcpus;
    let mut max_vcpus = direct_max_vcpus
        .or(instance_type.as_ref().map(|it| it.max_vcpus))
        .or(vm_template.as_ref().and_then(|template| template.max_vcpus))
        .unwrap_or(boot_vcpus);
    if let Some(direct_max_vcpus) = direct_max_vcpus {
        if direct_max_vcpus < boot_vcpus {
            return Err(Error::UnprocessableEntity(
                "max_vcpus must be greater than or equal to boot_vcpus".into(),
            ));
        }
    } else if max_vcpus < boot_vcpus {
        max_vcpus = boot_vcpus;
    }

    let hypervisor = hypervisor
        .or(vm_template
            .as_ref()
            .and_then(|template| template.hypervisor.clone()))
        .ok_or_else(|| {
            Error::UnprocessableEntity(
                "hypervisor is required unless provided by the selected VM template".into(),
            )
        })?;

    let memory_size = memory_size
        .or(instance_type.as_ref().map(|it| it.memory_size))
        .or(vm_template.as_ref().and_then(|template| template.memory_size))
        .ok_or_else(|| {
            Error::UnprocessableEntity(
                "memory_size is required unless provided by the selected instance type or VM template"
                    .into(),
            )
        })?;

    Ok(ResolvedNewVm {
        name,
        tags: tags.unwrap_or_default(),
        hypervisor,
        architecture,
        boot_vcpus,
        max_vcpus,
        cpu_topology: cpu_topology
            .or_else(|| {
                instance_type
                    .as_ref()
                    .and_then(|it| it.cpu_topology.clone())
            })
            .or_else(|| {
                vm_template
                    .as_ref()
                    .and_then(|template| template.cpu_topology.clone())
            }),
        kvm_hyperv: kvm_hyperv
            .or_else(|| instance_type.as_ref().and_then(|it| it.kvm_hyperv))
            .or_else(|| {
                vm_template
                    .as_ref()
                    .and_then(|template| template.kvm_hyperv)
            }),
        memory_size,
        memory_hotplug_size: memory_hotplug_size
            .or_else(|| instance_type.as_ref().and_then(|it| it.memory_hotplug_size))
            .or_else(|| {
                vm_template
                    .as_ref()
                    .and_then(|template| template.memory_hotplug_size)
            }),
        memory_mergeable: memory_mergeable
            .or_else(|| instance_type.as_ref().and_then(|it| it.memory_mergeable))
            .or_else(|| {
                vm_template
                    .as_ref()
                    .and_then(|template| template.memory_mergeable)
            }),
        memory_shared: memory_shared
            .or_else(|| instance_type.as_ref().and_then(|it| it.memory_shared))
            .or_else(|| {
                vm_template
                    .as_ref()
                    .and_then(|template| template.memory_shared)
            }),
        memory_hugepages: memory_hugepages
            .or_else(|| instance_type.as_ref().and_then(|it| it.memory_hugepages))
            .or_else(|| {
                vm_template
                    .as_ref()
                    .and_then(|template| template.memory_hugepages)
            }),
        memory_hugepage_size: memory_hugepage_size
            .or_else(|| {
                instance_type
                    .as_ref()
                    .and_then(|it| it.memory_hugepage_size)
            })
            .or_else(|| {
                vm_template
                    .as_ref()
                    .and_then(|template| template.memory_hugepage_size)
            }),
        memory_prefault: memory_prefault
            .or_else(|| instance_type.as_ref().and_then(|it| it.memory_prefault))
            .or_else(|| {
                vm_template
                    .as_ref()
                    .and_then(|template| template.memory_prefault)
            }),
        memory_thp: memory_thp
            .or_else(|| instance_type.as_ref().and_then(|it| it.memory_thp))
            .or_else(|| {
                vm_template
                    .as_ref()
                    .and_then(|template| template.memory_thp)
            }),
        boot_source_id: boot_source_id.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.boot_source_id)
        }),
        root_disk_object_id: root_disk_object_id.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.root_disk_object_id)
        }),
        boot_mode: boot_mode.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.boot_mode.clone())
        }),
        description: description.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.description.clone())
        }),
        image_ref: image_ref.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.image_ref.clone())
        }),
        cloud_init_user_data: cloud_init_user_data.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.cloud_init_user_data.clone())
        }),
        cloud_init_meta_data: cloud_init_meta_data.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.cloud_init_meta_data.clone())
        }),
        cloud_init_network_config: cloud_init_network_config.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.cloud_init_network_config.clone())
        }),
        network_id: network_id.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.network_id)
        }),
        networks: networks.or_else(|| {
            vm_template
                .as_ref()
                .and_then(|template| template.networks.clone())
        }),
        security_group_ids,
        accelerator_config: accelerator_config.or_else(|| {
            instance_type.as_ref().and_then(|it| {
                let v = &it.accelerator_config;
                if v.is_null() || v.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                    None
                } else {
                    Some(v.clone())
                }
            })
        }),
        numa_config: numa_config.or_else(|| {
            instance_type
                .as_ref()
                .and_then(|it| it.numa_config.clone())
                .filter(|v| !v.is_null())
        }),
        persistent_upper_pool_id,
        // NOTE: numa_config is intentionally not merged into `config` here;
        // the handler merges it in create_vm_internal before persisting.
        config: merge_config(
            vm_template
                .as_ref()
                .map(|template| template.config.clone())
                .unwrap_or_else(default_vm_config),
            config,
        ),
    })
}

pub async fn list(
    pool: &PgPool,
    name_filter: Option<&str>,
    tags_filter: &[String],
) -> Result<Vec<Vm>, sqlx::Error> {
    let vms: Vec<VmRow> = sqlx::query_as!(
        VmRow,
        r#"
SELECT id,
        name,
        tags as "tags!",
        status as "status: _",
        host_id as "host_id?",
        hypervisor as "hypervisor: _",
        boot_source_id as "boot_source_id?",
        boot_mode as "boot_mode: _",
        description as "description?",
        boot_vcpus,
        max_vcpus,
        cpu_topology as "cpu_topology: _",
        kvm_hyperv as "kvm_hyperv!",
        memory_size,
        memory_hotplug_size as "memory_hotplug_size?",
        memory_mergeable as "memory_mergeable!",
        memory_shared as "memory_shared!",
        memory_hugepages as "memory_hugepages!",
        memory_hugepage_size as "memory_hugepage_size?",
        memory_prefault as "memory_prefault!",
        memory_thp as "memory_thp!",
        image_ref as "image_ref?",
        cloud_init_user_data as "cloud_init_user_data?",
        cloud_init_meta_data as "cloud_init_meta_data?",
        cloud_init_network_config as "cloud_init_network_config?",
        config as "config: _"
FROM vms
WHERE ($1::text IS NULL OR name = $1)
  AND (cardinality($2::text[]) = 0 OR tags @> $2)
        "#,
        name_filter,
        tags_filter
    )
    .fetch_all(pool)
    .await?;

    let vms: Vec<Vm> = vms.into_iter().map(|vm: VmRow| vm.into()).collect();
    Ok(vms)
}

pub async fn get(pool: &PgPool, vm_id: Uuid) -> Result<Vm, sqlx::Error> {
    let vm: VmRow = sqlx::query_as!(
        VmRow,
        r#"
SELECT id,
        name,
        tags as "tags!",
        status as "status: _",
        host_id as "host_id?",
        hypervisor as "hypervisor: _",
        boot_source_id as "boot_source_id?",
        boot_mode as "boot_mode: _",
        description as "description?",
        boot_vcpus,
        max_vcpus,
        cpu_topology as "cpu_topology: _",
        kvm_hyperv as "kvm_hyperv!",
        memory_size,
        memory_hotplug_size as "memory_hotplug_size?",
        memory_mergeable as "memory_mergeable!",
        memory_shared as "memory_shared!",
        memory_hugepages as "memory_hugepages!",
        memory_hugepage_size as "memory_hugepage_size?",
        memory_prefault as "memory_prefault!",
        memory_thp as "memory_thp!",
        image_ref as "image_ref?",
        cloud_init_user_data as "cloud_init_user_data?",
        cloud_init_meta_data as "cloud_init_meta_data?",
        cloud_init_network_config as "cloud_init_network_config?",
        config as "config: _"
FROM vms
WHERE id = $1
        "#,
        vm_id
    )
    .fetch_one(pool)
    .await?;

    Ok(vm.into())
}

pub async fn create(pool: &PgPool, vm: &ResolvedNewVm) -> Result<Uuid, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let id = create_tx(&mut tx, vm, None).await?;
    tx.commit().await?;
    Ok(id)
}

/// Creates a VM row inside the given transaction. Used by the handler to roll back on node failure.
pub async fn create_tx(
    tx: &mut Transaction<'_, Postgres>,
    vm: &ResolvedNewVm,
    host_id: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    create_tx_with_status(tx, vm, host_id, VmStatus::Created).await
}

/// Creates a VM row with a specific initial status inside the given transaction.
pub async fn create_tx_with_status(
    tx: &mut Transaction<'_, Postgres>,
    vm: &ResolvedNewVm,
    host_id: Option<Uuid>,
    status: VmStatus,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let cpu_topology = vm.cpu_topology.as_ref().map(|t| Json(t.clone()));
    let config = Json(&vm.config);

    sqlx::query(
        r#"
INSERT INTO vms (
    id, name, tags, status, host_id, hypervisor,
    boot_vcpus, max_vcpus, cpu_topology, kvm_hyperv,
    memory_size, memory_hotplug_size, memory_mergeable, memory_shared,
    memory_hugepages, memory_hugepage_size, memory_prefault, memory_thp,
    boot_source_id, boot_mode, description, image_ref,
    cloud_init_user_data, cloud_init_meta_data, cloud_init_network_config,
    config
)
VALUES (
    $1, $2, $3, $4, $5, $6,
    $7, $8, $9, $10,
    $11, $12, $13, $14,
    $15, $16, $17, $18,
    $19, $20, $21, $22,
    $23, $24, $25,
    $26
)
        "#,
    )
    .bind(id)
    .bind(&vm.name)
    .bind(&vm.tags)
    .bind(status)
    .bind(host_id)
    .bind(vm.hypervisor.clone())
    .bind(vm.boot_vcpus)
    .bind(vm.max_vcpus)
    .bind(cpu_topology)
    .bind(vm.kvm_hyperv.unwrap_or(false))
    .bind(vm.memory_size)
    .bind(vm.memory_hotplug_size)
    .bind(vm.memory_mergeable.unwrap_or(false))
    .bind(vm.memory_shared.unwrap_or(false))
    .bind(vm.memory_hugepages.unwrap_or(false))
    .bind(vm.memory_hugepage_size)
    .bind(vm.memory_prefault.unwrap_or(false))
    .bind(vm.memory_thp.unwrap_or(false))
    .bind(vm.boot_source_id)
    .bind(vm.boot_mode.clone().unwrap_or(BootMode::Kernel))
    .bind(&vm.description)
    .bind(&vm.image_ref)
    .bind(&vm.cloud_init_user_data)
    .bind(&vm.cloud_init_meta_data)
    .bind(&vm.cloud_init_network_config)
    .bind(config)
    .execute(tx.as_mut())
    .await?;

    Ok(id)
}

pub async fn list_active(pool: &PgPool) -> Result<Vec<Vm>, sqlx::Error> {
    let vms = sqlx::query_as::<_, VmRow>(
        r#"
SELECT id,
        name,
        tags,
        status,
        host_id,
        hypervisor,
        boot_source_id,
        boot_mode,
        description,
        boot_vcpus,
        max_vcpus,
        cpu_topology,
        kvm_hyperv,
        memory_size,
        memory_hotplug_size,
        memory_mergeable,
        memory_shared,
        memory_hugepages,
        memory_hugepage_size,
        memory_prefault,
        memory_thp,
        image_ref,
        cloud_init_user_data,
        cloud_init_meta_data,
        cloud_init_network_config,
        config
FROM vms
WHERE status NOT IN ('SHUTDOWN', 'UNKNOWN', 'PENDING')
  AND host_id IS NOT NULL
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(vms.into_iter().map(|vm| vm.into()).collect())
}

pub async fn update_status(
    pool: &PgPool,
    vm_id: Uuid,
    status: VmStatus,
) -> Result<(), sqlx::Error> {
    // Fetch the VM before the update so we have previous status + metadata for hooks
    let vm = get(pool, vm_id).await.ok();

    sqlx::query("UPDATE vms SET status = $1 WHERE id = $2")
        .bind(&status)
        .bind(vm_id)
        .execute(pool)
        .await?;

    // Enqueue lifecycle hooks and emit SSE event (best-effort — don't fail the status update)
    if let Some(vm) = vm
        && vm.status != status
    {
        let new_status_str = status.to_string();
        let prev_status_str = vm.status.to_string();

        super::events::emit(
            vm.id,
            &vm.name,
            &prev_status_str,
            &new_status_str,
            vm.host_id,
            &vm.tags,
        );

        if let Err(e) =
            super::lifecycle_hooks::enqueue_matching(pool, &vm, &prev_status_str, &new_status_str)
                .await
        {
            tracing::warn!("failed to enqueue lifecycle hooks for VM {}: {}", vm_id, e);
        }
    }

    Ok(())
}

/// Atomically set a VM's status to Committing if it is currently in a
/// committable state (Created or Shutdown). Returns `true` if the transition
/// succeeded, `false` if the VM was in a different state (another commit or
/// start is already in progress).
pub async fn try_set_committing(pool: &PgPool, vm_id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE vms SET status = 'COMMITTING' WHERE id = $1 AND status IN ('CREATED', 'SHUTDOWN')",
    )
    .bind(vm_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn clear_image_ref(pool: &PgPool, vm_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE vms SET image_ref = NULL WHERE id = $1")
        .bind(vm_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_host_id(pool: &PgPool, vm_id: Uuid, host_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE vms SET host_id = $1 WHERE id = $2")
        .bind(host_id)
        .bind(vm_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn update_name(pool: &PgPool, vm_id: Uuid, name: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE vms SET name = $1 WHERE id = $2")
        .bind(name)
        .bind(vm_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_name_tx(
    tx: &mut Transaction<'_, Postgres>,
    vm_id: Uuid,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE vms SET name = $1 WHERE id = $2")
        .bind(name)
        .bind(vm_id)
        .execute(tx.as_mut())
        .await?;
    Ok(())
}

/// Update boot_vcpus and/or memory_size in a single query.
/// At least one of the two options must be `Some`.
pub async fn update_resize(
    pool: &PgPool,
    vm_id: Uuid,
    boot_vcpus: Option<i32>,
    memory_size: Option<i64>,
) -> Result<(), sqlx::Error> {
    match (boot_vcpus, memory_size) {
        (Some(vcpus), Some(mem)) => {
            sqlx::query("UPDATE vms SET boot_vcpus = $1, memory_size = $2 WHERE id = $3")
                .bind(vcpus)
                .bind(mem)
                .bind(vm_id)
                .execute(pool)
                .await?;
        }
        (Some(vcpus), None) => {
            sqlx::query("UPDATE vms SET boot_vcpus = $1 WHERE id = $2")
                .bind(vcpus)
                .bind(vm_id)
                .execute(pool)
                .await?;
        }
        (None, Some(mem)) => {
            sqlx::query("UPDATE vms SET memory_size = $1 WHERE id = $2")
                .bind(mem)
                .bind(vm_id)
                .execute(pool)
                .await?;
        }
        (None, None) => {}
    }
    Ok(())
}

pub async fn delete(pool: &PgPool, vm_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM vms WHERE id = $1")
        .bind(vm_id)
        .execute(pool)
        .await?;

    Ok(())
}
