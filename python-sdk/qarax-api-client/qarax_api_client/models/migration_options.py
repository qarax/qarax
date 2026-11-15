from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.migration_memory_mode import MigrationMemoryMode
from ..models.migration_timeout_action import MigrationTimeoutAction
from ..types import UNSET, Unset

T = TypeVar("T", bound="MigrationOptions")


@_attrs_define
class MigrationOptions:
    """Live migration tuning. Cloud Hypervisor's defaults apply to whatever is
    left out.

        Attributes:
            connections (int | None | Unset): Parallel TCP connections for the memory transfer (default 1).
            max_downtime_ms (int | None | Unset): Longest pause of the guest at switch-over to aim for (default 300 ms).
            memory_mode (MigrationMemoryMode | Unset): How a live migration moves guest memory.
            on_timeout (MigrationTimeoutAction | Unset): What a live migration does when it reaches its timeout.
            timeout_secs (int | None | Unset): Longest the whole migration may take (default 3600 s).
    """

    connections: int | None | Unset = UNSET
    max_downtime_ms: int | None | Unset = UNSET
    memory_mode: MigrationMemoryMode | Unset = UNSET
    on_timeout: MigrationTimeoutAction | Unset = UNSET
    timeout_secs: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        connections: int | None | Unset
        if isinstance(self.connections, Unset):
            connections = UNSET
        else:
            connections = self.connections

        max_downtime_ms: int | None | Unset
        if isinstance(self.max_downtime_ms, Unset):
            max_downtime_ms = UNSET
        else:
            max_downtime_ms = self.max_downtime_ms

        memory_mode: str | Unset = UNSET
        if not isinstance(self.memory_mode, Unset):
            memory_mode = self.memory_mode.value

        on_timeout: str | Unset = UNSET
        if not isinstance(self.on_timeout, Unset):
            on_timeout = self.on_timeout.value

        timeout_secs: int | None | Unset
        if isinstance(self.timeout_secs, Unset):
            timeout_secs = UNSET
        else:
            timeout_secs = self.timeout_secs

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if connections is not UNSET:
            field_dict["connections"] = connections
        if max_downtime_ms is not UNSET:
            field_dict["max_downtime_ms"] = max_downtime_ms
        if memory_mode is not UNSET:
            field_dict["memory_mode"] = memory_mode
        if on_timeout is not UNSET:
            field_dict["on_timeout"] = on_timeout
        if timeout_secs is not UNSET:
            field_dict["timeout_secs"] = timeout_secs

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_connections(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        connections = _parse_connections(d.pop("connections", UNSET))

        def _parse_max_downtime_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        max_downtime_ms = _parse_max_downtime_ms(d.pop("max_downtime_ms", UNSET))

        _memory_mode = d.pop("memory_mode", UNSET)
        memory_mode: MigrationMemoryMode | Unset
        if isinstance(_memory_mode, Unset):
            memory_mode = UNSET
        else:
            memory_mode = MigrationMemoryMode(_memory_mode)

        _on_timeout = d.pop("on_timeout", UNSET)
        on_timeout: MigrationTimeoutAction | Unset
        if isinstance(_on_timeout, Unset):
            on_timeout = UNSET
        else:
            on_timeout = MigrationTimeoutAction(_on_timeout)

        def _parse_timeout_secs(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        timeout_secs = _parse_timeout_secs(d.pop("timeout_secs", UNSET))

        migration_options = cls(
            connections=connections,
            max_downtime_ms=max_downtime_ms,
            memory_mode=memory_mode,
            on_timeout=on_timeout,
            timeout_secs=timeout_secs,
        )

        migration_options.additional_properties = d
        return migration_options

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
