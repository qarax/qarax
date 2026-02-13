from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.host_status import HostStatus

T = TypeVar("T", bound="Host")


@_attrs_define
class Host:
    """
    Attributes:
        address (str):
        host_user (str):
        id (UUID):
        name (str):
        port (int):
        status (HostStatus):
    """

    address: str
    host_user: str
    id: UUID
    name: str
    port: int
    status: HostStatus
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        address = self.address

        host_user = self.host_user

        id = str(self.id)

        name = self.name

        port = self.port

        status = self.status.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "address": address,
                "host_user": host_user,
                "id": id,
                "name": name,
                "port": port,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        address = d.pop("address")

        host_user = d.pop("host_user")

        id = UUID(d.pop("id"))

        name = d.pop("name")

        port = d.pop("port")

        status = HostStatus(d.pop("status"))

        host = cls(
            address=address,
            host_user=host_user,
            id=id,
            name=name,
            port=port,
            status=status,
        )

        host.additional_properties = d
        return host

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
