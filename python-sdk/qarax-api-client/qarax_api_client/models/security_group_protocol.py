from enum import StrEnum


class SecurityGroupProtocol(StrEnum):
    ANY = "any"
    ICMP = "icmp"
    TCP = "tcp"
    UDP = "udp"

    def __str__(self) -> str:
        return str(self.value)
