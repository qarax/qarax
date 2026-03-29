from __future__ import annotations

from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="AcceleratorConfig")


@_attrs_define
class AcceleratorConfig:
    """Typed accelerator_config from instance types / VM requests

    Attributes:
        gpu_count (int):
        gpu_model (None | str | Unset):
        gpu_vendor (None | str | Unset):
        min_vram_bytes (int | None | Unset):
        prefer_local_numa (bool | Unset): When true (default), pin the VM to the NUMA node(s) local to its allocated
            GPU(s).
    """

    gpu_count: int
    gpu_model: None | str | Unset = UNSET
    gpu_vendor: None | str | Unset = UNSET
    min_vram_bytes: int | None | Unset = UNSET
    prefer_local_numa: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        gpu_count = self.gpu_count

        gpu_model: None | str | Unset
        if isinstance(self.gpu_model, Unset):
            gpu_model = UNSET
        else:
            gpu_model = self.gpu_model

        gpu_vendor: None | str | Unset
        if isinstance(self.gpu_vendor, Unset):
            gpu_vendor = UNSET
        else:
            gpu_vendor = self.gpu_vendor

        min_vram_bytes: int | None | Unset
        if isinstance(self.min_vram_bytes, Unset):
            min_vram_bytes = UNSET
        else:
            min_vram_bytes = self.min_vram_bytes

        prefer_local_numa = self.prefer_local_numa

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "gpu_count": gpu_count,
            }
        )
        if gpu_model is not UNSET:
            field_dict["gpu_model"] = gpu_model
        if gpu_vendor is not UNSET:
            field_dict["gpu_vendor"] = gpu_vendor
        if min_vram_bytes is not UNSET:
            field_dict["min_vram_bytes"] = min_vram_bytes
        if prefer_local_numa is not UNSET:
            field_dict["prefer_local_numa"] = prefer_local_numa

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        gpu_count = d.pop("gpu_count")

        def _parse_gpu_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        gpu_model = _parse_gpu_model(d.pop("gpu_model", UNSET))

        def _parse_gpu_vendor(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        gpu_vendor = _parse_gpu_vendor(d.pop("gpu_vendor", UNSET))

        def _parse_min_vram_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        min_vram_bytes = _parse_min_vram_bytes(d.pop("min_vram_bytes", UNSET))

        prefer_local_numa = d.pop("prefer_local_numa", UNSET)

        accelerator_config = cls(
            gpu_count=gpu_count,
            gpu_model=gpu_model,
            gpu_vendor=gpu_vendor,
            min_vram_bytes=min_vram_bytes,
            prefer_local_numa=prefer_local_numa,
        )

        accelerator_config.additional_properties = d
        return accelerator_config

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
