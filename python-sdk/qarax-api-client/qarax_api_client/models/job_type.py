from enum import Enum


class JobType(str, Enum):
    DISK_CREATE = "disk_create"
    HOST_EVACUATE = "host_evacuate"
    IMAGE_PULL = "image_pull"
    SANDBOX_CLAIM = "sandbox_claim"
    VM_COMMIT = "vm_commit"
    VM_MIGRATE = "vm_migrate"
    VM_START = "vm_start"

    def __str__(self) -> str:
        return str(self.value)
