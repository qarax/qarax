from __future__ import annotations

import datetime
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from dateutil.parser import isoparse

from ..types import UNSET, Unset

T = TypeVar("T", bound="HostGpu")


@_attrs_define
class HostGpu:
    """
    Attributes:
        host_id (UUID):
        id (UUID):
        iommu_group (int):
        numa_node (int):
        pci_address (str):
        discovered_at (datetime.datetime | None | Unset):
        model (None | str | Unset):
        updated_at (datetime.datetime | None | Unset):
        vendor (None | str | Unset):
        vm_id (None | Unset | UUID):
        vram_bytes (int | None | Unset):
    """

    host_id: UUID
    id: UUID
    iommu_group: int
    numa_node: int
    pci_address: str
    discovered_at: datetime.datetime | None | Unset = UNSET
    model: None | str | Unset = UNSET
    updated_at: datetime.datetime | None | Unset = UNSET
    vendor: None | str | Unset = UNSET
    vm_id: None | Unset | UUID = UNSET
    vram_bytes: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        host_id = str(self.host_id)

        id = str(self.id)

        iommu_group = self.iommu_group

        numa_node = self.numa_node

        pci_address = self.pci_address

        discovered_at: None | str | Unset
        if isinstance(self.discovered_at, Unset):
            discovered_at = UNSET
        elif isinstance(self.discovered_at, datetime.datetime):
            discovered_at = self.discovered_at.isoformat()
        else:
            discovered_at = self.discovered_at

        model: None | str | Unset
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        updated_at: None | str | Unset
        if isinstance(self.updated_at, Unset):
            updated_at = UNSET
        elif isinstance(self.updated_at, datetime.datetime):
            updated_at = self.updated_at.isoformat()
        else:
            updated_at = self.updated_at

        vendor: None | str | Unset
        if isinstance(self.vendor, Unset):
            vendor = UNSET
        else:
            vendor = self.vendor

        vm_id: None | str | Unset
        if isinstance(self.vm_id, Unset):
            vm_id = UNSET
        elif isinstance(self.vm_id, UUID):
            vm_id = str(self.vm_id)
        else:
            vm_id = self.vm_id

        vram_bytes: int | None | Unset
        if isinstance(self.vram_bytes, Unset):
            vram_bytes = UNSET
        else:
            vram_bytes = self.vram_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "host_id": host_id,
                "id": id,
                "iommu_group": iommu_group,
                "numa_node": numa_node,
                "pci_address": pci_address,
            }
        )
        if discovered_at is not UNSET:
            field_dict["discovered_at"] = discovered_at
        if model is not UNSET:
            field_dict["model"] = model
        if updated_at is not UNSET:
            field_dict["updated_at"] = updated_at
        if vendor is not UNSET:
            field_dict["vendor"] = vendor
        if vm_id is not UNSET:
            field_dict["vm_id"] = vm_id
        if vram_bytes is not UNSET:
            field_dict["vram_bytes"] = vram_bytes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        host_id = UUID(d.pop("host_id"))

        id = UUID(d.pop("id"))

        iommu_group = d.pop("iommu_group")

        numa_node = d.pop("numa_node")

        pci_address = d.pop("pci_address")

        def _parse_discovered_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                discovered_at_type_0 = isoparse(data)

                return discovered_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        discovered_at = _parse_discovered_at(d.pop("discovered_at", UNSET))

        def _parse_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model = _parse_model(d.pop("model", UNSET))

        def _parse_updated_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                updated_at_type_0 = isoparse(data)

                return updated_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        updated_at = _parse_updated_at(d.pop("updated_at", UNSET))

        def _parse_vendor(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        vendor = _parse_vendor(d.pop("vendor", UNSET))

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

        def _parse_vram_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        vram_bytes = _parse_vram_bytes(d.pop("vram_bytes", UNSET))

        host_gpu = cls(
            host_id=host_id,
            id=id,
            iommu_group=iommu_group,
            numa_node=numa_node,
            pci_address=pci_address,
            discovered_at=discovered_at,
            model=model,
            updated_at=updated_at,
            vendor=vendor,
            vm_id=vm_id,
            vram_bytes=vram_bytes,
        )

        host_gpu.additional_properties = d
        return host_gpu

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
