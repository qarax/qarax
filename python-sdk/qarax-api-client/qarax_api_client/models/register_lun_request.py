from __future__ import annotations

from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RegisterLunRequest")


@_attrs_define
class RegisterLunRequest:
    """
    Attributes:
        lun (int): LUN number exported by the iSCSI target for this disk.
        name (str): Human-readable name for the resulting storage object.
        size_bytes (int): Logical size of the LUN in bytes. Informational (reported back to clients).
    """

    lun: int
    name: str
    size_bytes: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        lun = self.lun

        name = self.name

        size_bytes = self.size_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "lun": lun,
                "name": name,
                "size_bytes": size_bytes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        lun = d.pop("lun")

        name = d.pop("name")

        size_bytes = d.pop("size_bytes")

        register_lun_request = cls(
            lun=lun,
            name=name,
            size_bytes=size_bytes,
        )

        register_lun_request.additional_properties = d
        return register_lun_request

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
