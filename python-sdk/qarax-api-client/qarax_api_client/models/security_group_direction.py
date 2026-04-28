from enum import Enum


class SecurityGroupDirection(str, Enum):
    EGRESS = "egress"
    INGRESS = "ingress"

    def __str__(self) -> str:
        return str(self.value)
