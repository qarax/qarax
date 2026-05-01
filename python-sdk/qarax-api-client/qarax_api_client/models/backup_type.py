from enum import Enum


class BackupType(str, Enum):
    DATABASE = "database"
    VM = "vm"

    def __str__(self) -> str:
        return str(self.value)
