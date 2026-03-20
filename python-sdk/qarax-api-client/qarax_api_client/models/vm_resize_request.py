from __future__ import annotations

from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="VmResizeRequest")


@_attrs_define
class VmResizeRequest:
    """Request body for `PUT /vms/{vm_id}/resize`.

    At least one of `desired_vcpus` or `desired_ram` must be provided.
    - `desired_vcpus` must be in the range `[boot_vcpus, max_vcpus]`.
    - `desired_ram` must be in the range `[memory_size, memory_size + memory_hotplug_size]`.

        Attributes:
            desired_ram (int | None | Unset): Target memory size in bytes
            desired_vcpus (int | None | Unset): Target vCPU count
    """

    desired_ram: int | None | Unset = UNSET
    desired_vcpus: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        desired_ram: int | None | Unset
        if isinstance(self.desired_ram, Unset):
            desired_ram = UNSET
        else:
            desired_ram = self.desired_ram

        desired_vcpus: int | None | Unset
        if isinstance(self.desired_vcpus, Unset):
            desired_vcpus = UNSET
        else:
            desired_vcpus = self.desired_vcpus

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if desired_ram is not UNSET:
            field_dict["desired_ram"] = desired_ram
        if desired_vcpus is not UNSET:
            field_dict["desired_vcpus"] = desired_vcpus

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)

        def _parse_desired_ram(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        desired_ram = _parse_desired_ram(d.pop("desired_ram", UNSET))

        def _parse_desired_vcpus(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        desired_vcpus = _parse_desired_vcpus(d.pop("desired_vcpus", UNSET))

        vm_resize_request = cls(
            desired_ram=desired_ram,
            desired_vcpus=desired_vcpus,
        )

        vm_resize_request.additional_properties = d
        return vm_resize_request

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
