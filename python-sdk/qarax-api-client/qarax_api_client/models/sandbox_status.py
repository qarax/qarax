from enum import StrEnum


class SandboxStatus(StrEnum):
    DESTROYING = "destroying"
    ERROR = "error"
    PROVISIONING = "provisioning"
    READY = "ready"

    def __str__(self) -> str:
        return str(self.value)
