from enum import StrEnum


class VmStatus(StrEnum):
    COMMITTING = "committing"
    CREATED = "created"
    MIGRATING = "migrating"
    PAUSED = "paused"
    PENDING = "pending"
    RUNNING = "running"
    SHUTDOWN = "shutdown"
    UNKNOWN = "unknown"

    def __str__(self) -> str:
        return str(self.value)
