from enum import StrEnum


class SnapshotStatus(StrEnum):
    CREATING = "creating"
    FAILED = "failed"
    READY = "ready"

    def __str__(self) -> str:
        return str(self.value)
