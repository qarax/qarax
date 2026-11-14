from enum import StrEnum


class BackupType(StrEnum):
    DATABASE = "database"
    VM = "vm"

    def __str__(self) -> str:
        return str(self.value)
