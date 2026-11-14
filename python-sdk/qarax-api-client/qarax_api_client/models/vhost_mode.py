from enum import StrEnum


class VhostMode(StrEnum):
    CLIENT = "client"
    SERVER = "server"

    def __str__(self) -> str:
        return str(self.value)
