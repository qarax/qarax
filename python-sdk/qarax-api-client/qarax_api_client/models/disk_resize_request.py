from __future__ import annotations

from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="DiskResizeRequest")


@_attrs_define
class DiskResizeRequest:
    """Request body for `PUT /vms/{vm_id}/disks/{disk_id}/resize`.

    Attributes:
        new_size_bytes (int): New disk size in bytes. Must be larger than the current size and a multiple of 1 MiB.
    """

    new_size_bytes: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        new_size_bytes = self.new_size_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "new_size_bytes": new_size_bytes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        new_size_bytes = d.pop("new_size_bytes")

        disk_resize_request = cls(
            new_size_bytes=new_size_bytes,
        )

        disk_resize_request.additional_properties = d
        return disk_resize_request

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
