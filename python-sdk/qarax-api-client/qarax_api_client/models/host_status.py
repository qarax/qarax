from enum import StrEnum


class HostStatus(StrEnum):
    DOWN = "down"
    INITIALIZING = "initializing"
    INSTALLATION_FAILED = "installation_failed"
    INSTALLING = "installing"
    MAINTENANCE = "maintenance"
    UNKNOWN = "unknown"
    UP = "up"

    def __str__(self) -> str:
        return str(self.value)
