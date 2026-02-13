from enum import Enum


class Hypervisor(str, Enum):
    CLOUD_HV = "cloud_hv"

    def __str__(self) -> str:
        return str(self.value)
