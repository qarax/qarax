from enum import Enum


class HostStatus(str, Enum):
    DOWN = "down"
    INITIALIZING = "initializing"
    INSTALLATION_FAILED = "installation_failed"
    INSTALLING = "installing"
    UNKNOWN = "unknown"
    UP = "up"

    def __str__(self) -> str:
        return str(self.value)
