from enum import StrEnum


class SecurityGroupDirection(StrEnum):
    EGRESS = "egress"
    INGRESS = "ingress"

    def __str__(self) -> str:
        return str(self.value)
