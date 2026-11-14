from enum import StrEnum


class HookScope(StrEnum):
    GLOBAL = "global"
    TAG = "tag"
    VM = "vm"

    def __str__(self) -> str:
        return str(self.value)
