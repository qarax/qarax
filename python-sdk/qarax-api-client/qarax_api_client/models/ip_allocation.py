from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="IpAllocation")


@_attrs_define
class IpAllocation:
    """
    Attributes:
        allocated_at (datetime.datetime):
        id (UUID):
        ip_address (str):
        network_id (UUID):
        vm_id (None | Unset | UUID):
    """

    allocated_at: datetime.datetime
    id: UUID
    ip_address: str
    network_id: UUID
    vm_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        allocated_at = self.allocated_at.isoformat()

        id = str(self.id)

        ip_address = self.ip_address

        network_id = str(self.network_id)

        vm_id: None | str | Unset
        if isinstance(self.vm_id, Unset):
            vm_id = UNSET
        elif isinstance(self.vm_id, UUID):
            vm_id = str(self.vm_id)
        else:
            vm_id = self.vm_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "allocated_at": allocated_at,
                "id": id,
                "ip_address": ip_address,
                "network_id": network_id,
            }
        )
        if vm_id is not UNSET:
            field_dict["vm_id"] = vm_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        allocated_at = datetime.datetime.fromisoformat(d.pop("allocated_at"))

        id = UUID(d.pop("id"))

        ip_address = d.pop("ip_address")

        network_id = UUID(d.pop("network_id"))

        def _parse_vm_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                vm_id_type_0 = UUID(data)

                return vm_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        vm_id = _parse_vm_id(d.pop("vm_id", UNSET))

        ip_allocation = cls(
            allocated_at=allocated_at,
            id=id,
            ip_address=ip_address,
            network_id=network_id,
            vm_id=vm_id,
        )

        ip_allocation.additional_properties = d
        return ip_allocation

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
