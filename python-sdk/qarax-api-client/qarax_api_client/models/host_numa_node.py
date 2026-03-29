from __future__ import annotations

import datetime
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from dateutil.parser import isoparse

from ..types import UNSET, Unset

T = TypeVar("T", bound="HostNumaNode")


@_attrs_define
class HostNumaNode:
    """
    Attributes:
        cpu_list (str):
        distances (list[int]):
        host_id (UUID):
        id (UUID):
        node_id (int):
        memory_bytes (int | None | Unset):
        updated_at (datetime.datetime | None | Unset):
    """

    cpu_list: str
    distances: list[int]
    host_id: UUID
    id: UUID
    node_id: int
    memory_bytes: int | None | Unset = UNSET
    updated_at: datetime.datetime | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cpu_list = self.cpu_list

        distances = self.distances

        host_id = str(self.host_id)

        id = str(self.id)

        node_id = self.node_id

        memory_bytes: int | None | Unset
        if isinstance(self.memory_bytes, Unset):
            memory_bytes = UNSET
        else:
            memory_bytes = self.memory_bytes

        updated_at: None | str | Unset
        if isinstance(self.updated_at, Unset):
            updated_at = UNSET
        elif isinstance(self.updated_at, datetime.datetime):
            updated_at = self.updated_at.isoformat()
        else:
            updated_at = self.updated_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "cpu_list": cpu_list,
                "distances": distances,
                "host_id": host_id,
                "id": id,
                "node_id": node_id,
            }
        )
        if memory_bytes is not UNSET:
            field_dict["memory_bytes"] = memory_bytes
        if updated_at is not UNSET:
            field_dict["updated_at"] = updated_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        cpu_list = d.pop("cpu_list")

        distances = cast(list[int], d.pop("distances"))

        host_id = UUID(d.pop("host_id"))

        id = UUID(d.pop("id"))

        node_id = d.pop("node_id")

        def _parse_memory_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        memory_bytes = _parse_memory_bytes(d.pop("memory_bytes", UNSET))

        def _parse_updated_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                updated_at_type_0 = isoparse(data)

                return updated_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        updated_at = _parse_updated_at(d.pop("updated_at", UNSET))

        host_numa_node = cls(
            cpu_list=cpu_list,
            distances=distances,
            host_id=host_id,
            id=id,
            node_id=node_id,
            memory_bytes=memory_bytes,
            updated_at=updated_at,
        )

        host_numa_node.additional_properties = d
        return host_numa_node

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
