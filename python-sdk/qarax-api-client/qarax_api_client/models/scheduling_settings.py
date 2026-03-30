from __future__ import annotations

from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="SchedulingSettings")


@_attrs_define
class SchedulingSettings:
    """
    Attributes:
        cpu_oversubscription_ratio (float | Unset):
        disk_headroom_bytes (int | Unset):
        memory_health_floor_bytes (int | Unset):
        memory_oversubscription_ratio (float | Unset):
    """

    cpu_oversubscription_ratio: float | Unset = UNSET
    disk_headroom_bytes: int | Unset = UNSET
    memory_health_floor_bytes: int | Unset = UNSET
    memory_oversubscription_ratio: float | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cpu_oversubscription_ratio = self.cpu_oversubscription_ratio

        disk_headroom_bytes = self.disk_headroom_bytes

        memory_health_floor_bytes = self.memory_health_floor_bytes

        memory_oversubscription_ratio = self.memory_oversubscription_ratio

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if cpu_oversubscription_ratio is not UNSET:
            field_dict["cpu_oversubscription_ratio"] = cpu_oversubscription_ratio
        if disk_headroom_bytes is not UNSET:
            field_dict["disk_headroom_bytes"] = disk_headroom_bytes
        if memory_health_floor_bytes is not UNSET:
            field_dict["memory_health_floor_bytes"] = memory_health_floor_bytes
        if memory_oversubscription_ratio is not UNSET:
            field_dict["memory_oversubscription_ratio"] = memory_oversubscription_ratio

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        cpu_oversubscription_ratio = d.pop("cpu_oversubscription_ratio", UNSET)

        disk_headroom_bytes = d.pop("disk_headroom_bytes", UNSET)

        memory_health_floor_bytes = d.pop("memory_health_floor_bytes", UNSET)

        memory_oversubscription_ratio = d.pop("memory_oversubscription_ratio", UNSET)

        scheduling_settings = cls(
            cpu_oversubscription_ratio=cpu_oversubscription_ratio,
            disk_headroom_bytes=disk_headroom_bytes,
            memory_health_floor_bytes=memory_health_floor_bytes,
            memory_oversubscription_ratio=memory_oversubscription_ratio,
        )

        scheduling_settings.additional_properties = d
        return scheduling_settings

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
