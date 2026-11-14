from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.storage_pool_type import StoragePoolType
from ..types import UNSET, Unset

T = TypeVar("T", bound="NewStoragePool")


@_attrs_define
class NewStoragePool:
    """
    Attributes:
        name (str):
        pool_type (StoragePoolType):
        capacity_bytes (int | None | Unset):
        config (Any | Unset):
    """

    name: str
    pool_type: StoragePoolType
    capacity_bytes: int | None | Unset = UNSET
    config: Any | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        pool_type = self.pool_type.value

        capacity_bytes: int | None | Unset
        if isinstance(self.capacity_bytes, Unset):
            capacity_bytes = UNSET
        else:
            capacity_bytes = self.capacity_bytes

        config = self.config

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "pool_type": pool_type,
            }
        )
        if capacity_bytes is not UNSET:
            field_dict["capacity_bytes"] = capacity_bytes
        if config is not UNSET:
            field_dict["config"] = config

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        pool_type = StoragePoolType(d.pop("pool_type"))

        def _parse_capacity_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        capacity_bytes = _parse_capacity_bytes(d.pop("capacity_bytes", UNSET))

        config = d.pop("config", UNSET)

        new_storage_pool = cls(
            name=name,
            pool_type=pool_type,
            capacity_bytes=capacity_bytes,
            config=config,
        )

        new_storage_pool.additional_properties = d
        return new_storage_pool

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
