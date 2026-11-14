from enum import StrEnum


class TransferType(StrEnum):
    DOWNLOAD = "download"
    LOCAL_COPY = "local_copy"

    def __str__(self) -> str:
        return str(self.value)
