from enum import Enum


class VmStatus(str, Enum):
    CREATED = "created"
    PAUSED = "paused"
    RUNNING = "running"
    SHUTDOWN = "shutdown"
    UNKNOWN = "unknown"

    def __str__(self) -> str:
        return str(self.value)
