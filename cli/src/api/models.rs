use serde::{Deserialize, Serialize};
use serde_with::rust::double_option;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Vm {
    pub id: Uuid,
    pub name: String,
    pub tags: Vec<String>,
    pub host_id: Option<Uuid>,
    pub status: String,
    pub hypervisor: String,
    pub boot_source_id: Option<Uuid>,
    pub boot_mode: String,
    pub description: Option<String>,
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub memory_size: i64,
    pub image_ref: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewVmNetwork {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload_tso: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload_ufo: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload_csum: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct NewVm {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm_template_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_type_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hypervisor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_vcpus: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_vcpus: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_disk_object_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub networks: Option<Vec<NewVmNetwork>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_init_user_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_init_meta_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_init_network_config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accelerator_config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstanceType {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub memory_size: i64,
}

#[derive(Debug, Serialize)]
pub struct NewInstanceType {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub boot_vcpus: i32,
    pub max_vcpus: i32,
    pub memory_size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accelerator_config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VmTemplate {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub hypervisor: Option<String>,
    pub boot_vcpus: Option<i32>,
    pub max_vcpus: Option<i32>,
    pub memory_size: Option<i64>,
    pub boot_source_id: Option<Uuid>,
    pub root_disk_object_id: Option<Uuid>,
    pub boot_mode: Option<String>,
    pub image_ref: Option<String>,
    pub network_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct NewVmTemplate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hypervisor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_vcpus: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_vcpus: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_disk_object_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct CreateVmTemplateFromVmRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateVmResponse {
    pub vm_id: Uuid,
    pub job_id: Uuid,
}

pub enum CreateVmResult {
    Created(Uuid),
    Accepted { vm_id: Uuid, job_id: Uuid },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VmStartResponse {
    pub job_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VmMigrateRequest {
    pub target_host_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VmMigrateResponse {
    pub job_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub port: i32,
    pub status: String,
    pub host_user: String,
    pub cloud_hypervisor_version: Option<String>,
    pub kernel_version: Option<String>,
    pub node_version: Option<String>,
    pub last_deployed_image: Option<String>,
    pub update_available: bool,
    pub total_cpus: Option<i32>,
    pub total_memory_bytes: Option<i64>,
    pub available_memory_bytes: Option<i64>,
    pub load_average: Option<f64>,
    pub disk_total_bytes: Option<i64>,
    pub disk_available_bytes: Option<i64>,
    pub resources_updated_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewHost {
    pub name: String,
    pub address: String,
    pub port: i32,
    pub host_user: String,
    pub password: String,
}

#[derive(Debug, Serialize, Default)]
pub struct DeployHostRequest {
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_private_key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_bootc: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reboot: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HostGpu {
    pub id: Uuid,
    pub host_id: Uuid,
    pub pci_address: String,
    pub model: Option<String>,
    pub vendor: Option<String>,
    pub vram_bytes: Option<i64>,
    pub iommu_group: i32,
    pub vm_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoragePool {
    pub id: Uuid,
    pub name: String,
    pub pool_type: String,
    pub status: String,
    pub capacity_bytes: Option<i64>,
    pub allocated_bytes: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct NewStoragePool {
    pub name: String,
    pub pool_type: String,
    pub config: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_bytes: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AttachHostToPoolRequest {
    pub host_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageObject {
    pub id: Uuid,
    pub name: String,
    pub storage_pool_id: Uuid,
    pub object_type: String,
    pub size_bytes: i64,
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct NewStorageObject {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_pool_id: Option<Uuid>,
    pub object_type: String,
    pub size_bytes: i64,
    pub config: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Transfer {
    pub id: Uuid,
    pub name: String,
    pub transfer_type: String,
    pub status: String,
    pub source: String,
    pub storage_pool_id: Uuid,
    pub object_type: String,
    pub storage_object_id: Option<Uuid>,
    pub total_bytes: Option<i64>,
    pub transferred_bytes: i64,
    pub error_message: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewTransfer {
    pub name: String,
    pub source: String,
    pub object_type: String,
}

// ─── Boot sources ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct BootSource {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub kernel_image_id: Uuid,
    pub kernel_params: Option<String>,
    pub initrd_image_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct NewBootSource {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub kernel_image_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel_params: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initrd_image_id: Option<Uuid>,
}

// ─── Networks ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Network {
    pub id: Uuid,
    pub name: String,
    pub subnet: String,
    pub gateway: Option<String>,
    pub dns: Option<String>,
    #[serde(rename = "type", alias = "network_type")]
    pub network_type: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct NewNetwork {
    pub name: String,
    pub subnet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub network_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AttachHostToNetworkRequest {
    pub host_id: Uuid,
    pub bridge_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_interface: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpAllocation {
    pub id: Uuid,
    pub network_id: Uuid,
    pub ip_address: String,
    pub vm_id: Option<Uuid>,
    pub allocated_at: String,
}

// ─── Jobs ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub description: Option<String>,
    pub resource_id: Option<Uuid>,
    pub progress: Option<i32>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ─── VM resize ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct VmResizeRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_vcpus: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_ram: Option<i64>,
}

// ─── VM disks ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AttachDiskRequest {
    pub storage_object_id: Uuid,
    pub logical_name: Option<String>,
    pub boot_order: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VmDisk {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub storage_object_id: Option<Uuid>,
    pub logical_name: String,
    pub device_path: String,
    pub boot_order: Option<i32>,
    pub read_only: bool,
    pub direct: bool,
    pub vhost_user: bool,
    pub vhost_socket: Option<String>,
    pub num_queues: i32,
    pub queue_size: i32,
    pub rate_limiter: Option<serde_json::Value>,
    pub rate_limit_group: Option<String>,
    pub pci_segment: i32,
    pub serial_number: Option<String>,
    pub config: serde_json::Value,
}

// ─── Network interfaces ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub network_id: Option<Uuid>,
    pub device_id: String,
    pub tap_name: Option<String>,
    pub mac_address: Option<String>,
    pub ip_address: Option<String>,
    pub mtu: i32,
    pub interface_type: String,
    pub num_queues: i32,
    pub queue_size: i32,
    pub offload_tso: bool,
    pub offload_ufo: bool,
    pub offload_csum: bool,
}

#[derive(Debug, Serialize)]
pub struct HotplugNicRequest {
    /// Unique device ID for the new NIC (e.g. "net1"). Auto-generated if empty.
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtu: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload_tso: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload_ufo: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload_csum: Option<bool>,
}

// ─── Snapshots ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub storage_object_id: Uuid,
    pub name: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct CreateSnapshotRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_pool_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct RestoreRequest {
    pub snapshot_id: Uuid,
}

// ─── Storage pool import ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportToPoolRequest {
    pub name: String,
    pub image_ref: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportToPoolResponse {
    pub job_id: Uuid,
    pub storage_object_id: Uuid,
}

// ─── Sandboxes ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Sandbox {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub vm_template_id: Option<Uuid>,
    pub name: String,
    pub status: String,
    pub idle_timeout_secs: i32,
    pub last_activity_at: String,
    pub error_message: Option<String>,
    pub created_at: String,
    pub ip_address: Option<String>,
    pub vm_status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewSandbox {
    pub name: String,
    pub vm_template_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_timeout_secs: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_type_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSandboxResponse {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub job_id: Uuid,
}

// ─── Lifecycle Hooks ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct LifecycleHook {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    pub scope: String,
    pub scope_value: Option<String>,
    pub events: Vec<String>,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct NewLifecycleHook {
    pub name: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_value: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateLifecycleHook {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(
        default,
        with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub secret: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(
        default,
        with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub scope_value: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HookExecution {
    pub id: Uuid,
    pub hook_id: Uuid,
    pub vm_id: Uuid,
    pub previous_status: String,
    pub new_status: String,
    pub status: String,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub payload: serde_json::Value,
    pub response_status: Option<i32>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub delivered_at: Option<String>,
}
