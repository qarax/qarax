from enum import StrEnum


class BootMode(StrEnum):
    FIRMWARE = "firmware"
    KERNEL = "kernel"

    def __str__(self) -> str:
        return str(self.value)
