from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="AttachHostRequest")


@_attrs_define
class AttachHostRequest:
    """
    Attributes:
        bridge_name (str):
        host_id (UUID):
        parent_interface (None | str | Unset):
    """

    bridge_name: str
    host_id: UUID
    parent_interface: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        bridge_name = self.bridge_name

        host_id = str(self.host_id)

        parent_interface: None | str | Unset
        if isinstance(self.parent_interface, Unset):
            parent_interface = UNSET
        else:
            parent_interface = self.parent_interface

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "bridge_name": bridge_name,
                "host_id": host_id,
            }
        )
        if parent_interface is not UNSET:
            field_dict["parent_interface"] = parent_interface

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        bridge_name = d.pop("bridge_name")

        host_id = UUID(d.pop("host_id"))

        def _parse_parent_interface(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        parent_interface = _parse_parent_interface(d.pop("parent_interface", UNSET))

        attach_host_request = cls(
            bridge_name=bridge_name,
            host_id=host_id,
            parent_interface=parent_interface,
        )

        attach_host_request.additional_properties = d
        return attach_host_request

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
