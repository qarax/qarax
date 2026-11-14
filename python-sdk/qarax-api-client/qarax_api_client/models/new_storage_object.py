from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.storage_object_type import StorageObjectType
from ..types import UNSET, Unset

T = TypeVar("T", bound="NewStorageObject")


@_attrs_define
class NewStorageObject:
    """
    Attributes:
        name (str):
        object_type (StorageObjectType):
        size_bytes (int):
        config (Any | Unset):
        parent_id (None | Unset | UUID):
        storage_pool_id (None | Unset | UUID): Storage pool to place this object in. If omitted, a random active pool is
            chosen.
    """

    name: str
    object_type: StorageObjectType
    size_bytes: int
    config: Any | Unset = UNSET
    parent_id: None | Unset | UUID = UNSET
    storage_pool_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        object_type = self.object_type.value

        size_bytes = self.size_bytes

        config = self.config

        parent_id: None | str | Unset
        if isinstance(self.parent_id, Unset):
            parent_id = UNSET
        elif isinstance(self.parent_id, UUID):
            parent_id = str(self.parent_id)
        else:
            parent_id = self.parent_id

        storage_pool_id: None | str | Unset
        if isinstance(self.storage_pool_id, Unset):
            storage_pool_id = UNSET
        elif isinstance(self.storage_pool_id, UUID):
            storage_pool_id = str(self.storage_pool_id)
        else:
            storage_pool_id = self.storage_pool_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "object_type": object_type,
                "size_bytes": size_bytes,
            }
        )
        if config is not UNSET:
            field_dict["config"] = config
        if parent_id is not UNSET:
            field_dict["parent_id"] = parent_id
        if storage_pool_id is not UNSET:
            field_dict["storage_pool_id"] = storage_pool_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        object_type = StorageObjectType(d.pop("object_type"))

        size_bytes = d.pop("size_bytes")

        config = d.pop("config", UNSET)

        def _parse_parent_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                parent_id_type_0 = UUID(data)

                return parent_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        parent_id = _parse_parent_id(d.pop("parent_id", UNSET))

        def _parse_storage_pool_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                storage_pool_id_type_0 = UUID(data)

                return storage_pool_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        storage_pool_id = _parse_storage_pool_id(d.pop("storage_pool_id", UNSET))

        new_storage_object = cls(
            name=name,
            object_type=object_type,
            size_bytes=size_bytes,
            config=config,
            parent_id=parent_id,
            storage_pool_id=storage_pool_id,
        )

        new_storage_object.additional_properties = d
        return new_storage_object

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
