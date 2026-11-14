from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.storage_object_type import StorageObjectType

T = TypeVar("T", bound="NewTransfer")


@_attrs_define
class NewTransfer:
    """
    Attributes:
        name (str):
        object_type (StorageObjectType):
        source (str):
    """

    name: str
    object_type: StorageObjectType
    source: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        object_type = self.object_type.value

        source = self.source

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "object_type": object_type,
                "source": source,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        object_type = StorageObjectType(d.pop("object_type"))

        source = d.pop("source")

        new_transfer = cls(
            name=name,
            object_type=object_type,
            source=source,
        )

        new_transfer.additional_properties = d
        return new_transfer

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
