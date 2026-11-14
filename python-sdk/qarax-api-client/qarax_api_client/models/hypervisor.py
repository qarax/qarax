from enum import StrEnum


class Hypervisor(StrEnum):
    CLOUD_HV = "cloud_hv"
    FIRECRACKER = "firecracker"

    def __str__(self) -> str:
        return str(self.value)
