from __future__ import annotations

from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="HostResourceCapacity")


@_attrs_define
class HostResourceCapacity:
    """
    Attributes:
        allocated_memory_bytes (int):
        allocated_vcpus (int):
        host_id (UUID):
        architecture (None | str | Unset):
        available_memory_bytes (int | None | Unset):
        disk_available_bytes (int | None | Unset):
        disk_total_bytes (int | None | Unset):
        total_cpus (int | None | Unset):
        total_memory_bytes (int | None | Unset):
    """

    allocated_memory_bytes: int
    allocated_vcpus: int
    host_id: UUID
    architecture: None | str | Unset = UNSET
    available_memory_bytes: int | None | Unset = UNSET
    disk_available_bytes: int | None | Unset = UNSET
    disk_total_bytes: int | None | Unset = UNSET
    total_cpus: int | None | Unset = UNSET
    total_memory_bytes: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        allocated_memory_bytes = self.allocated_memory_bytes

        allocated_vcpus = self.allocated_vcpus

        host_id = str(self.host_id)

        architecture: None | str | Unset
        if isinstance(self.architecture, Unset):
            architecture = UNSET
        else:
            architecture = self.architecture

        available_memory_bytes: int | None | Unset
        if isinstance(self.available_memory_bytes, Unset):
            available_memory_bytes = UNSET
        else:
            available_memory_bytes = self.available_memory_bytes

        disk_available_bytes: int | None | Unset
        if isinstance(self.disk_available_bytes, Unset):
            disk_available_bytes = UNSET
        else:
            disk_available_bytes = self.disk_available_bytes

        disk_total_bytes: int | None | Unset
        if isinstance(self.disk_total_bytes, Unset):
            disk_total_bytes = UNSET
        else:
            disk_total_bytes = self.disk_total_bytes

        total_cpus: int | None | Unset
        if isinstance(self.total_cpus, Unset):
            total_cpus = UNSET
        else:
            total_cpus = self.total_cpus

        total_memory_bytes: int | None | Unset
        if isinstance(self.total_memory_bytes, Unset):
            total_memory_bytes = UNSET
        else:
            total_memory_bytes = self.total_memory_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "allocated_memory_bytes": allocated_memory_bytes,
                "allocated_vcpus": allocated_vcpus,
                "host_id": host_id,
            }
        )
        if architecture is not UNSET:
            field_dict["architecture"] = architecture
        if available_memory_bytes is not UNSET:
            field_dict["available_memory_bytes"] = available_memory_bytes
        if disk_available_bytes is not UNSET:
            field_dict["disk_available_bytes"] = disk_available_bytes
        if disk_total_bytes is not UNSET:
            field_dict["disk_total_bytes"] = disk_total_bytes
        if total_cpus is not UNSET:
            field_dict["total_cpus"] = total_cpus
        if total_memory_bytes is not UNSET:
            field_dict["total_memory_bytes"] = total_memory_bytes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        allocated_memory_bytes = d.pop("allocated_memory_bytes")

        allocated_vcpus = d.pop("allocated_vcpus")

        host_id = UUID(d.pop("host_id"))

        def _parse_architecture(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        architecture = _parse_architecture(d.pop("architecture", UNSET))

        def _parse_available_memory_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        available_memory_bytes = _parse_available_memory_bytes(d.pop("available_memory_bytes", UNSET))

        def _parse_disk_available_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        disk_available_bytes = _parse_disk_available_bytes(d.pop("disk_available_bytes", UNSET))

        def _parse_disk_total_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        disk_total_bytes = _parse_disk_total_bytes(d.pop("disk_total_bytes", UNSET))

        def _parse_total_cpus(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        total_cpus = _parse_total_cpus(d.pop("total_cpus", UNSET))

        def _parse_total_memory_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        total_memory_bytes = _parse_total_memory_bytes(d.pop("total_memory_bytes", UNSET))

        host_resource_capacity = cls(
            allocated_memory_bytes=allocated_memory_bytes,
            allocated_vcpus=allocated_vcpus,
            host_id=host_id,
            architecture=architecture,
            available_memory_bytes=available_memory_bytes,
            disk_available_bytes=disk_available_bytes,
            disk_total_bytes=disk_total_bytes,
            total_cpus=total_cpus,
            total_memory_bytes=total_memory_bytes,
        )

        host_resource_capacity.additional_properties = d
        return host_resource_capacity

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
