from enum import StrEnum


class StoragePoolType(StrEnum):
    BLOCK = "block"
    LOCAL = "local"
    NFS = "nfs"
    OVERLAY_BD = "overlay_bd"

    def __str__(self) -> str:
        return str(self.value)
