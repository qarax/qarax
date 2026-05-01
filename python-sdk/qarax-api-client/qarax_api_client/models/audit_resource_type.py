from enum import Enum


class AuditResourceType(str, Enum):
    BACKUP = "backup"
    BOOT_SOURCE = "boot_source"
    HOST = "host"
    INSTANCE_TYPE = "instance_type"
    LIFECYCLE_HOOK = "lifecycle_hook"
    NETWORK = "network"
    SANDBOX = "sandbox"
    SECURITY_GROUP = "security_group"
    STORAGE_OBJECT = "storage_object"
    STORAGE_POOL = "storage_pool"
    TRANSFER = "transfer"
    VM = "vm"
    VM_TEMPLATE = "vm_template"

    def __str__(self) -> str:
        return str(self.value)
