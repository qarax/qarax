from enum import StrEnum


class MigrationMemoryMode(StrEnum):
    POSTCOPY = "postcopy"
    PRECOPY = "precopy"

    def __str__(self) -> str:
        return str(self.value)
