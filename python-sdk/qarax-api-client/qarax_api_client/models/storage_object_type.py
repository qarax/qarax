from enum import Enum


class StorageObjectType(str, Enum):
    DISK = "disk"
    INITRD = "initrd"
    ISO = "iso"
    KERNEL = "kernel"
    OCI_IMAGE = "oci_image"
    OVERLAYBD_UPPER = "overlaybd_upper"
    SNAPSHOT = "snapshot"

    def __str__(self) -> str:
        return str(self.value)
