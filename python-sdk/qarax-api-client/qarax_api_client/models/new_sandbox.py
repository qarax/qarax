from __future__ import annotations

from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="NewSandbox")


@_attrs_define
class NewSandbox:
    """
    Attributes:
        name (str):
        vm_template_id (UUID):
        idle_timeout_secs (int | None | Unset):
        instance_type_id (None | Unset | UUID):
        network_id (None | Unset | UUID):
    """

    name: str
    vm_template_id: UUID
    idle_timeout_secs: int | None | Unset = UNSET
    instance_type_id: None | Unset | UUID = UNSET
    network_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        vm_template_id = str(self.vm_template_id)

        idle_timeout_secs: int | None | Unset
        if isinstance(self.idle_timeout_secs, Unset):
            idle_timeout_secs = UNSET
        else:
            idle_timeout_secs = self.idle_timeout_secs

        instance_type_id: None | str | Unset
        if isinstance(self.instance_type_id, Unset):
            instance_type_id = UNSET
        elif isinstance(self.instance_type_id, UUID):
            instance_type_id = str(self.instance_type_id)
        else:
            instance_type_id = self.instance_type_id

        network_id: None | str | Unset
        if isinstance(self.network_id, Unset):
            network_id = UNSET
        elif isinstance(self.network_id, UUID):
            network_id = str(self.network_id)
        else:
            network_id = self.network_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "vm_template_id": vm_template_id,
            }
        )
        if idle_timeout_secs is not UNSET:
            field_dict["idle_timeout_secs"] = idle_timeout_secs
        if instance_type_id is not UNSET:
            field_dict["instance_type_id"] = instance_type_id
        if network_id is not UNSET:
            field_dict["network_id"] = network_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        vm_template_id = UUID(d.pop("vm_template_id"))

        def _parse_idle_timeout_secs(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        idle_timeout_secs = _parse_idle_timeout_secs(d.pop("idle_timeout_secs", UNSET))

        def _parse_instance_type_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                instance_type_id_type_0 = UUID(data)

                return instance_type_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        instance_type_id = _parse_instance_type_id(d.pop("instance_type_id", UNSET))

        def _parse_network_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                network_id_type_0 = UUID(data)

                return network_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        network_id = _parse_network_id(d.pop("network_id", UNSET))

        new_sandbox = cls(
            name=name,
            vm_template_id=vm_template_id,
            idle_timeout_secs=idle_timeout_secs,
            instance_type_id=instance_type_id,
            network_id=network_id,
        )

        new_sandbox.additional_properties = d
        return new_sandbox

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
