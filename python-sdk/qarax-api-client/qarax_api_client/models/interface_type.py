from enum import StrEnum


class InterfaceType(StrEnum):
    MACVTAP = "macvtap"
    TAP = "tap"
    VHOST_USER = "vhost_user"

    def __str__(self) -> str:
        return str(self.value)
