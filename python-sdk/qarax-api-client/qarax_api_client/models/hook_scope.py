from enum import Enum


class HookScope(str, Enum):
    GLOBAL = "global"
    TAG = "tag"
    VM = "vm"

    def __str__(self) -> str:
        return str(self.value)
