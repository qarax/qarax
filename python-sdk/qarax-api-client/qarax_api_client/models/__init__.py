"""Contains all the data models used in inputs/outputs"""

from .accelerator_config import AcceleratorConfig
from .attach_disk_request import AttachDiskRequest
from .attach_host_request import AttachHostRequest
from .attach_pool_host_request import AttachPoolHostRequest
from .boot_mode import BootMode
from .boot_source import BootSource
from .create_sandbox_response import CreateSandboxResponse
from .create_snapshot_request import CreateSnapshotRequest
from .create_vm_response import CreateVmResponse
from .create_vm_template_from_vm_request import CreateVmTemplateFromVmRequest
from .deploy_host_request import DeployHostRequest
from .disk_resize_request import DiskResizeRequest
from .exec_sandbox_request import ExecSandboxRequest
from .exec_sandbox_response import ExecSandboxResponse
from .hook_execution import HookExecution
from .hook_execution_status import HookExecutionStatus
from .hook_scope import HookScope
from .host import Host
from .host_gpu import HostGpu
from .host_numa_node import HostNumaNode
from .host_resource_capacity import HostResourceCapacity
from .host_status import HostStatus
from .hypervisor import Hypervisor
from .import_to_pool_request import ImportToPoolRequest
from .import_to_pool_response import ImportToPoolResponse
from .instance_type import InstanceType
from .interface_type import InterfaceType
from .ip_allocation import IpAllocation
from .job import Job
from .job_status import JobStatus
from .job_type import JobType
from .lifecycle_hook import LifecycleHook
from .network import Network
from .network_interface import NetworkInterface
from .network_status import NetworkStatus
from .new_boot_source import NewBootSource
from .new_host import NewHost
from .new_instance_type import NewInstanceType
from .new_lifecycle_hook import NewLifecycleHook
from .new_network import NewNetwork
from .new_sandbox import NewSandbox
from .new_storage_object import NewStorageObject
from .new_storage_pool import NewStoragePool
from .new_transfer import NewTransfer
from .new_vm import NewVm
from .new_vm_filesystem import NewVmFilesystem
from .new_vm_network import NewVmNetwork
from .new_vm_template import NewVmTemplate
from .rate_limiter_config import RateLimiterConfig
from .restore_request import RestoreRequest
from .sandbox import Sandbox
from .sandbox_status import SandboxStatus
from .scheduling_settings import SchedulingSettings
from .snapshot import Snapshot
from .snapshot_status import SnapshotStatus
from .storage_object import StorageObject
from .storage_object_type import StorageObjectType
from .storage_pool import StoragePool
from .storage_pool_status import StoragePoolStatus
from .storage_pool_type import StoragePoolType
from .token_bucket import TokenBucket
from .transfer import Transfer
from .transfer_status import TransferStatus
from .transfer_type import TransferType
from .update_host_request import UpdateHostRequest
from .update_lifecycle_hook import UpdateLifecycleHook
from .vhost_mode import VhostMode
from .vm import Vm
from .vm_disk import VmDisk
from .vm_filesystem import VmFilesystem
from .vm_metrics import VmMetrics
from .vm_metrics_counters import VmMetricsCounters
from .vm_metrics_counters_additional_property import VmMetricsCountersAdditionalProperty
from .vm_migrate_request import VmMigrateRequest
from .vm_migrate_response import VmMigrateResponse
from .vm_resize_request import VmResizeRequest
from .vm_start_response import VmStartResponse
from .vm_status import VmStatus
from .vm_template import VmTemplate

__all__ = (
    "AcceleratorConfig",
    "AttachDiskRequest",
    "AttachHostRequest",
    "AttachPoolHostRequest",
    "BootMode",
    "BootSource",
    "CreateSandboxResponse",
    "CreateSnapshotRequest",
    "CreateVmResponse",
    "CreateVmTemplateFromVmRequest",
    "DeployHostRequest",
    "DiskResizeRequest",
    "ExecSandboxRequest",
    "ExecSandboxResponse",
    "HookExecution",
    "HookExecutionStatus",
    "HookScope",
    "Host",
    "HostGpu",
    "HostNumaNode",
    "HostResourceCapacity",
    "HostStatus",
    "Hypervisor",
    "ImportToPoolRequest",
    "ImportToPoolResponse",
    "InstanceType",
    "InterfaceType",
    "IpAllocation",
    "Job",
    "JobStatus",
    "JobType",
    "LifecycleHook",
    "Network",
    "NetworkInterface",
    "NetworkStatus",
    "NewBootSource",
    "NewHost",
    "NewInstanceType",
    "NewLifecycleHook",
    "NewNetwork",
    "NewSandbox",
    "NewStorageObject",
    "NewStoragePool",
    "NewTransfer",
    "NewVm",
    "NewVmFilesystem",
    "NewVmNetwork",
    "NewVmTemplate",
    "RateLimiterConfig",
    "RestoreRequest",
    "Sandbox",
    "SandboxStatus",
    "SchedulingSettings",
    "Snapshot",
    "SnapshotStatus",
    "StorageObject",
    "StorageObjectType",
    "StoragePool",
    "StoragePoolStatus",
    "StoragePoolType",
    "TokenBucket",
    "Transfer",
    "TransferStatus",
    "TransferType",
    "UpdateHostRequest",
    "UpdateLifecycleHook",
    "VhostMode",
    "Vm",
    "VmDisk",
    "VmFilesystem",
    "VmMetrics",
    "VmMetricsCounters",
    "VmMetricsCountersAdditionalProperty",
    "VmMigrateRequest",
    "VmMigrateResponse",
    "VmResizeRequest",
    "VmStartResponse",
    "VmStatus",
    "VmTemplate",
)
