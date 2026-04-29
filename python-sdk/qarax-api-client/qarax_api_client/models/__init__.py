"""Contains all the data models used in inputs/outputs"""

from .accelerator_config import AcceleratorConfig
from .attach_disk_request import AttachDiskRequest
from .attach_host_request import AttachHostRequest
from .attach_pool_host_request import AttachPoolHostRequest
from .attach_security_group_request import AttachSecurityGroupRequest
from .audit_action import AuditAction
from .audit_log import AuditLog
from .audit_resource_type import AuditResourceType
from .boot_mode import BootMode
from .boot_source import BootSource
from .commit_vm_request import CommitVmRequest
from .commit_vm_response import CommitVmResponse
from .configure_sandbox_pool_request import ConfigureSandboxPoolRequest
from .create_disk_request import CreateDiskRequest
from .create_disk_response import CreateDiskResponse
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
from .host_evacuate_response import HostEvacuateResponse
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
from .new_security_group import NewSecurityGroup
from .new_security_group_rule import NewSecurityGroupRule
from .new_storage_object import NewStorageObject
from .new_storage_pool import NewStoragePool
from .new_transfer import NewTransfer
from .new_vm import NewVm
from .new_vm_network import NewVmNetwork
from .new_vm_template import NewVmTemplate
from .rate_limiter_config import RateLimiterConfig
from .register_lun_request import RegisterLunRequest
from .restore_request import RestoreRequest
from .sandbox import Sandbox
from .sandbox_pool import SandboxPool
from .sandbox_status import SandboxStatus
from .scheduling_settings import SchedulingSettings
from .security_group import SecurityGroup
from .security_group_direction import SecurityGroupDirection
from .security_group_protocol import SecurityGroupProtocol
from .security_group_rule import SecurityGroupRule
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
from .vm_image_preflight_check import VmImagePreflightCheck
from .vm_image_preflight_request import VmImagePreflightRequest
from .vm_image_preflight_response import VmImagePreflightResponse
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
    "AttachSecurityGroupRequest",
    "AuditAction",
    "AuditLog",
    "AuditResourceType",
    "BootMode",
    "BootSource",
    "CommitVmRequest",
    "CommitVmResponse",
    "ConfigureSandboxPoolRequest",
    "CreateDiskRequest",
    "CreateDiskResponse",
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
    "HostEvacuateResponse",
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
    "NewSecurityGroup",
    "NewSecurityGroupRule",
    "NewStorageObject",
    "NewStoragePool",
    "NewTransfer",
    "NewVm",
    "NewVmNetwork",
    "NewVmTemplate",
    "RateLimiterConfig",
    "RegisterLunRequest",
    "RestoreRequest",
    "Sandbox",
    "SandboxPool",
    "SandboxStatus",
    "SchedulingSettings",
    "SecurityGroup",
    "SecurityGroupDirection",
    "SecurityGroupProtocol",
    "SecurityGroupRule",
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
    "VmImagePreflightCheck",
    "VmImagePreflightRequest",
    "VmImagePreflightResponse",
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
