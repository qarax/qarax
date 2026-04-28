from __future__ import annotations

from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="NewNetwork")


@_attrs_define
class NewNetwork:
    """
    Attributes:
        name (str):
        subnet (str):
        dns (None | str | Unset):
        gateway (None | str | Unset):
        type_ (None | str | Unset):
        vpc_name (None | str | Unset):
    """

    name: str
    subnet: str
    dns: None | str | Unset = UNSET
    gateway: None | str | Unset = UNSET
    type_: None | str | Unset = UNSET
    vpc_name: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        subnet = self.subnet

        dns: None | str | Unset
        if isinstance(self.dns, Unset):
            dns = UNSET
        else:
            dns = self.dns

        gateway: None | str | Unset
        if isinstance(self.gateway, Unset):
            gateway = UNSET
        else:
            gateway = self.gateway

        type_: None | str | Unset
        if isinstance(self.type_, Unset):
            type_ = UNSET
        else:
            type_ = self.type_

        vpc_name: None | str | Unset
        if isinstance(self.vpc_name, Unset):
            vpc_name = UNSET
        else:
            vpc_name = self.vpc_name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "subnet": subnet,
            }
        )
        if dns is not UNSET:
            field_dict["dns"] = dns
        if gateway is not UNSET:
            field_dict["gateway"] = gateway
        if type_ is not UNSET:
            field_dict["type"] = type_
        if vpc_name is not UNSET:
            field_dict["vpc_name"] = vpc_name

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        subnet = d.pop("subnet")

        def _parse_dns(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        dns = _parse_dns(d.pop("dns", UNSET))

        def _parse_gateway(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        gateway = _parse_gateway(d.pop("gateway", UNSET))

        def _parse_type_(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        type_ = _parse_type_(d.pop("type", UNSET))

        def _parse_vpc_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        vpc_name = _parse_vpc_name(d.pop("vpc_name", UNSET))

        new_network = cls(
            name=name,
            subnet=subnet,
            dns=dns,
            gateway=gateway,
            type_=type_,
            vpc_name=vpc_name,
        )

        new_network.additional_properties = d
        return new_network

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
