from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.vm_status import VmStatus
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.vm_metrics_counters import VmMetricsCounters


T = TypeVar("T", bound="VmMetrics")


@_attrs_define
class VmMetrics:
    """
    Attributes:
        counters (VmMetricsCounters):
        status (VmStatus):
        vm_id (UUID):
        memory_actual_size (int | None | Unset):
    """

    counters: VmMetricsCounters
    status: VmStatus
    vm_id: UUID
    memory_actual_size: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        counters = self.counters.to_dict()

        status = self.status.value

        vm_id = str(self.vm_id)

        memory_actual_size: int | None | Unset
        if isinstance(self.memory_actual_size, Unset):
            memory_actual_size = UNSET
        else:
            memory_actual_size = self.memory_actual_size

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "counters": counters,
                "status": status,
                "vm_id": vm_id,
            }
        )
        if memory_actual_size is not UNSET:
            field_dict["memory_actual_size"] = memory_actual_size

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.vm_metrics_counters import VmMetricsCounters  # noqa: PLC0415

        d = dict(src_dict)
        counters = VmMetricsCounters.from_dict(d.pop("counters"))

        status = VmStatus(d.pop("status"))

        vm_id = UUID(d.pop("vm_id"))

        def _parse_memory_actual_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        memory_actual_size = _parse_memory_actual_size(d.pop("memory_actual_size", UNSET))

        vm_metrics = cls(
            counters=counters,
            status=status,
            vm_id=vm_id,
            memory_actual_size=memory_actual_size,
        )

        vm_metrics.additional_properties = d
        return vm_metrics

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
