from __future__ import annotations

from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.security_group_direction import SecurityGroupDirection
from ..models.security_group_protocol import SecurityGroupProtocol
from ..types import UNSET, Unset

T = TypeVar("T", bound="SecurityGroupRule")


@_attrs_define
class SecurityGroupRule:
    """
    Attributes:
        direction (SecurityGroupDirection):
        id (UUID):
        protocol (SecurityGroupProtocol):
        security_group_id (UUID):
        cidr (None | str | Unset):
        description (None | str | Unset):
        port_end (int | None | Unset):
        port_start (int | None | Unset):
    """

    direction: SecurityGroupDirection
    id: UUID
    protocol: SecurityGroupProtocol
    security_group_id: UUID
    cidr: None | str | Unset = UNSET
    description: None | str | Unset = UNSET
    port_end: int | None | Unset = UNSET
    port_start: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        direction = self.direction.value

        id = str(self.id)

        protocol = self.protocol.value

        security_group_id = str(self.security_group_id)

        cidr: None | str | Unset
        if isinstance(self.cidr, Unset):
            cidr = UNSET
        else:
            cidr = self.cidr

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        port_end: int | None | Unset
        if isinstance(self.port_end, Unset):
            port_end = UNSET
        else:
            port_end = self.port_end

        port_start: int | None | Unset
        if isinstance(self.port_start, Unset):
            port_start = UNSET
        else:
            port_start = self.port_start

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "direction": direction,
                "id": id,
                "protocol": protocol,
                "security_group_id": security_group_id,
            }
        )
        if cidr is not UNSET:
            field_dict["cidr"] = cidr
        if description is not UNSET:
            field_dict["description"] = description
        if port_end is not UNSET:
            field_dict["port_end"] = port_end
        if port_start is not UNSET:
            field_dict["port_start"] = port_start

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        direction = SecurityGroupDirection(d.pop("direction"))

        id = UUID(d.pop("id"))

        protocol = SecurityGroupProtocol(d.pop("protocol"))

        security_group_id = UUID(d.pop("security_group_id"))

        def _parse_cidr(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cidr = _parse_cidr(d.pop("cidr", UNSET))

        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))

        def _parse_port_end(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        port_end = _parse_port_end(d.pop("port_end", UNSET))

        def _parse_port_start(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        port_start = _parse_port_start(d.pop("port_start", UNSET))

        security_group_rule = cls(
            direction=direction,
            id=id,
            protocol=protocol,
            security_group_id=security_group_id,
            cidr=cidr,
            description=description,
            port_end=port_end,
            port_start=port_start,
        )

        security_group_rule.additional_properties = d
        return security_group_rule

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
