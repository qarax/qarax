from enum import Enum


class JobType(str, Enum):
    DISK_CREATE = "disk_create"
    IMAGE_PULL = "image_pull"
    VM_MIGRATE = "vm_migrate"
    VM_START = "vm_start"

    def __str__(self) -> str:
        return str(self.value)
