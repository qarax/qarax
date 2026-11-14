from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.storage_pool_status import StoragePoolStatus
from ..models.storage_pool_type import StoragePoolType
from ..types import UNSET, Unset

T = TypeVar("T", bound="StoragePool")


@_attrs_define
class StoragePool:
    """
    Attributes:
        config (Any):
        id (UUID):
        name (str):
        pool_type (StoragePoolType):
        status (StoragePoolStatus):
        allocated_bytes (int | None | Unset):
        capacity_bytes (int | None | Unset):
    """

    config: Any
    id: UUID
    name: str
    pool_type: StoragePoolType
    status: StoragePoolStatus
    allocated_bytes: int | None | Unset = UNSET
    capacity_bytes: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        config = self.config

        id = str(self.id)

        name = self.name

        pool_type = self.pool_type.value

        status = self.status.value

        allocated_bytes: int | None | Unset
        if isinstance(self.allocated_bytes, Unset):
            allocated_bytes = UNSET
        else:
            allocated_bytes = self.allocated_bytes

        capacity_bytes: int | None | Unset
        if isinstance(self.capacity_bytes, Unset):
            capacity_bytes = UNSET
        else:
            capacity_bytes = self.capacity_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "config": config,
                "id": id,
                "name": name,
                "pool_type": pool_type,
                "status": status,
            }
        )
        if allocated_bytes is not UNSET:
            field_dict["allocated_bytes"] = allocated_bytes
        if capacity_bytes is not UNSET:
            field_dict["capacity_bytes"] = capacity_bytes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        config = d.pop("config")

        id = UUID(d.pop("id"))

        name = d.pop("name")

        pool_type = StoragePoolType(d.pop("pool_type"))

        status = StoragePoolStatus(d.pop("status"))

        def _parse_allocated_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        allocated_bytes = _parse_allocated_bytes(d.pop("allocated_bytes", UNSET))

        def _parse_capacity_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        capacity_bytes = _parse_capacity_bytes(d.pop("capacity_bytes", UNSET))

        storage_pool = cls(
            config=config,
            id=id,
            name=name,
            pool_type=pool_type,
            status=status,
            allocated_bytes=allocated_bytes,
            capacity_bytes=capacity_bytes,
        )

        storage_pool.additional_properties = d
        return storage_pool

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
