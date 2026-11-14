from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="AttachDiskRequest")


@_attrs_define
class AttachDiskRequest:
    """
    Attributes:
        storage_object_id (UUID): Storage object ID (any type: disk, oci_image, iso, etc.).
        boot_order (int | None | Unset): Boot priority — lower is higher priority (default: `0`).
        logical_name (None | str | Unset): Logical device name inside the VM (e.g. "vda"); auto-generated if omitted.
    """

    storage_object_id: UUID
    boot_order: int | None | Unset = UNSET
    logical_name: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        storage_object_id = str(self.storage_object_id)

        boot_order: int | None | Unset
        if isinstance(self.boot_order, Unset):
            boot_order = UNSET
        else:
            boot_order = self.boot_order

        logical_name: None | str | Unset
        if isinstance(self.logical_name, Unset):
            logical_name = UNSET
        else:
            logical_name = self.logical_name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "storage_object_id": storage_object_id,
            }
        )
        if boot_order is not UNSET:
            field_dict["boot_order"] = boot_order
        if logical_name is not UNSET:
            field_dict["logical_name"] = logical_name

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        storage_object_id = UUID(d.pop("storage_object_id"))

        def _parse_boot_order(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        boot_order = _parse_boot_order(d.pop("boot_order", UNSET))

        def _parse_logical_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        logical_name = _parse_logical_name(d.pop("logical_name", UNSET))

        attach_disk_request = cls(
            storage_object_id=storage_object_id,
            boot_order=boot_order,
            logical_name=logical_name,
        )

        attach_disk_request.additional_properties = d
        return attach_disk_request

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
