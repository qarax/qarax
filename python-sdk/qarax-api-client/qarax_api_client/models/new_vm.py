from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.hypervisor import Hypervisor
from ..types import UNSET, Unset

T = TypeVar("T", bound="NewVm")


@_attrs_define
class NewVm:
    """
    Attributes:
        boot_vcpus (int):
        hypervisor (Hypervisor):
        max_vcpus (int):
        memory_size (int):
        name (str):
        boot_source_id (None | Unset | UUID):
        config (Any | Unset):
        cpu_topology (Any | Unset):
        description (None | str | Unset):
        kvm_hyperv (bool | None | Unset):
        memory_hotplug_size (int | None | Unset):
        memory_hugepage_size (int | None | Unset):
        memory_hugepages (bool | None | Unset):
        memory_mergeable (bool | None | Unset):
        memory_prefault (bool | None | Unset):
        memory_shared (bool | None | Unset):
        memory_thp (bool | None | Unset):
    """

    boot_vcpus: int
    hypervisor: Hypervisor
    max_vcpus: int
    memory_size: int
    name: str
    boot_source_id: None | Unset | UUID = UNSET
    config: Any | Unset = UNSET
    cpu_topology: Any | Unset = UNSET
    description: None | str | Unset = UNSET
    kvm_hyperv: bool | None | Unset = UNSET
    memory_hotplug_size: int | None | Unset = UNSET
    memory_hugepage_size: int | None | Unset = UNSET
    memory_hugepages: bool | None | Unset = UNSET
    memory_mergeable: bool | None | Unset = UNSET
    memory_prefault: bool | None | Unset = UNSET
    memory_shared: bool | None | Unset = UNSET
    memory_thp: bool | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        boot_vcpus = self.boot_vcpus

        hypervisor = self.hypervisor.value

        max_vcpus = self.max_vcpus

        memory_size = self.memory_size

        name = self.name

        boot_source_id: None | str | Unset
        if isinstance(self.boot_source_id, Unset):
            boot_source_id = UNSET
        elif isinstance(self.boot_source_id, UUID):
            boot_source_id = str(self.boot_source_id)
        else:
            boot_source_id = self.boot_source_id

        config = self.config

        cpu_topology = self.cpu_topology

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        kvm_hyperv: bool | None | Unset
        if isinstance(self.kvm_hyperv, Unset):
            kvm_hyperv = UNSET
        else:
            kvm_hyperv = self.kvm_hyperv

        memory_hotplug_size: int | None | Unset
        if isinstance(self.memory_hotplug_size, Unset):
            memory_hotplug_size = UNSET
        else:
            memory_hotplug_size = self.memory_hotplug_size

        memory_hugepage_size: int | None | Unset
        if isinstance(self.memory_hugepage_size, Unset):
            memory_hugepage_size = UNSET
        else:
            memory_hugepage_size = self.memory_hugepage_size

        memory_hugepages: bool | None | Unset
        if isinstance(self.memory_hugepages, Unset):
            memory_hugepages = UNSET
        else:
            memory_hugepages = self.memory_hugepages

        memory_mergeable: bool | None | Unset
        if isinstance(self.memory_mergeable, Unset):
            memory_mergeable = UNSET
        else:
            memory_mergeable = self.memory_mergeable

        memory_prefault: bool | None | Unset
        if isinstance(self.memory_prefault, Unset):
            memory_prefault = UNSET
        else:
            memory_prefault = self.memory_prefault

        memory_shared: bool | None | Unset
        if isinstance(self.memory_shared, Unset):
            memory_shared = UNSET
        else:
            memory_shared = self.memory_shared

        memory_thp: bool | None | Unset
        if isinstance(self.memory_thp, Unset):
            memory_thp = UNSET
        else:
            memory_thp = self.memory_thp

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "boot_vcpus": boot_vcpus,
                "hypervisor": hypervisor,
                "max_vcpus": max_vcpus,
                "memory_size": memory_size,
                "name": name,
            }
        )
        if boot_source_id is not UNSET:
            field_dict["boot_source_id"] = boot_source_id
        if config is not UNSET:
            field_dict["config"] = config
        if cpu_topology is not UNSET:
            field_dict["cpu_topology"] = cpu_topology
        if description is not UNSET:
            field_dict["description"] = description
        if kvm_hyperv is not UNSET:
            field_dict["kvm_hyperv"] = kvm_hyperv
        if memory_hotplug_size is not UNSET:
            field_dict["memory_hotplug_size"] = memory_hotplug_size
        if memory_hugepage_size is not UNSET:
            field_dict["memory_hugepage_size"] = memory_hugepage_size
        if memory_hugepages is not UNSET:
            field_dict["memory_hugepages"] = memory_hugepages
        if memory_mergeable is not UNSET:
            field_dict["memory_mergeable"] = memory_mergeable
        if memory_prefault is not UNSET:
            field_dict["memory_prefault"] = memory_prefault
        if memory_shared is not UNSET:
            field_dict["memory_shared"] = memory_shared
        if memory_thp is not UNSET:
            field_dict["memory_thp"] = memory_thp

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        boot_vcpus = d.pop("boot_vcpus")

        hypervisor = Hypervisor(d.pop("hypervisor"))

        max_vcpus = d.pop("max_vcpus")

        memory_size = d.pop("memory_size")

        name = d.pop("name")

        def _parse_boot_source_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                boot_source_id_type_0 = UUID(data)

                return boot_source_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        boot_source_id = _parse_boot_source_id(d.pop("boot_source_id", UNSET))

        config = d.pop("config", UNSET)

        cpu_topology = d.pop("cpu_topology", UNSET)

        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))

        def _parse_kvm_hyperv(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        kvm_hyperv = _parse_kvm_hyperv(d.pop("kvm_hyperv", UNSET))

        def _parse_memory_hotplug_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        memory_hotplug_size = _parse_memory_hotplug_size(d.pop("memory_hotplug_size", UNSET))

        def _parse_memory_hugepage_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        memory_hugepage_size = _parse_memory_hugepage_size(d.pop("memory_hugepage_size", UNSET))

        def _parse_memory_hugepages(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        memory_hugepages = _parse_memory_hugepages(d.pop("memory_hugepages", UNSET))

        def _parse_memory_mergeable(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        memory_mergeable = _parse_memory_mergeable(d.pop("memory_mergeable", UNSET))

        def _parse_memory_prefault(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        memory_prefault = _parse_memory_prefault(d.pop("memory_prefault", UNSET))

        def _parse_memory_shared(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        memory_shared = _parse_memory_shared(d.pop("memory_shared", UNSET))

        def _parse_memory_thp(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        memory_thp = _parse_memory_thp(d.pop("memory_thp", UNSET))

        new_vm = cls(
            boot_vcpus=boot_vcpus,
            hypervisor=hypervisor,
            max_vcpus=max_vcpus,
            memory_size=memory_size,
            name=name,
            boot_source_id=boot_source_id,
            config=config,
            cpu_topology=cpu_topology,
            description=description,
            kvm_hyperv=kvm_hyperv,
            memory_hotplug_size=memory_hotplug_size,
            memory_hugepage_size=memory_hugepage_size,
            memory_hugepages=memory_hugepages,
            memory_mergeable=memory_mergeable,
            memory_prefault=memory_prefault,
            memory_shared=memory_shared,
            memory_thp=memory_thp,
        )

        new_vm.additional_properties = d
        return new_vm

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
