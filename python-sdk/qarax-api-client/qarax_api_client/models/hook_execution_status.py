from enum import StrEnum


class HookExecutionStatus(StrEnum):
    DELIVERED = "delivered"
    FAILED = "failed"
    PENDING = "pending"
    PROCESSING = "processing"

    def __str__(self) -> str:
        return str(self.value)
