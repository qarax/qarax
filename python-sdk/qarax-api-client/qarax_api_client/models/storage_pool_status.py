from enum import StrEnum


class StoragePoolStatus(StrEnum):
    ACTIVE = "active"
    ERROR = "error"
    INACTIVE = "inactive"

    def __str__(self) -> str:
        return str(self.value)
