from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="NewHost")


@_attrs_define
class NewHost:
    """
    Attributes:
        address (str):
        host_user (str):
        name (str):
        password (str):
        port (int):
    """

    address: str
    host_user: str
    name: str
    password: str
    port: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        address = self.address

        host_user = self.host_user

        name = self.name

        password = self.password

        port = self.port

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "address": address,
                "host_user": host_user,
                "name": name,
                "password": password,
                "port": port,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        address = d.pop("address")

        host_user = d.pop("host_user")

        name = d.pop("name")

        password = d.pop("password")

        port = d.pop("port")

        new_host = cls(
            address=address,
            host_user=host_user,
            name=name,
            password=password,
            port=port,
        )

        new_host.additional_properties = d
        return new_host

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
