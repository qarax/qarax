from enum import Enum


class AuditAction(str, Enum):
    ADD_NIC = "add_nic"
    ATTACH_DISK = "attach_disk"
    COMMIT = "commit"
    CREATE = "create"
    CREATE_SNAPSHOT = "create_snapshot"
    CREATE_TEMPLATE = "create_template"
    DELETE = "delete"
    DEPLOY = "deploy"
    FORCE_STOP = "force_stop"
    INIT = "init"
    MIGRATE = "migrate"
    NODE_UPGRADE = "node_upgrade"
    PAUSE = "pause"
    REMOVE_DISK = "remove_disk"
    REMOVE_NIC = "remove_nic"
    RESIZE = "resize"
    RESTORE = "restore"
    RESTORE_SNAPSHOT = "restore_snapshot"
    RESUME = "resume"
    START = "start"
    STOP = "stop"
    UPDATE = "update"

    def __str__(self) -> str:
        return str(self.value)
