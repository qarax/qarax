from enum import Enum


class SandboxStatus(str, Enum):
    DESTROYING = "destroying"
    ERROR = "error"
    PROVISIONING = "provisioning"
    READY = "ready"

    def __str__(self) -> str:
        return str(self.value)
