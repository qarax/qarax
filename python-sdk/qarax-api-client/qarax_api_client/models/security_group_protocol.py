from enum import Enum


class SecurityGroupProtocol(str, Enum):
    ANY = "any"
    ICMP = "icmp"
    TCP = "tcp"
    UDP = "udp"

    def __str__(self) -> str:
        return str(self.value)
