from enum import StrEnum


class MigrationTimeoutAction(StrEnum):
    CANCEL = "cancel"
    FINISH = "finish"

    def __str__(self) -> str:
        return str(self.value)
