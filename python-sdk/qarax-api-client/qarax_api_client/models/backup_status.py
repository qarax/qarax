from enum import StrEnum


class BackupStatus(StrEnum):
    CREATING = "creating"
    FAILED = "failed"
    READY = "ready"

    def __str__(self) -> str:
        return str(self.value)
