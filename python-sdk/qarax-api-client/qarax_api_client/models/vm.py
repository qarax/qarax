from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.boot_mode import BootMode
from ..models.hypervisor import Hypervisor
from ..models.vm_status import VmStatus
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.placement_policy import PlacementPolicy


T = TypeVar("T", bound="Vm")


@_attrs_define
class Vm:
    """
    Attributes:
        boot_mode (BootMode):
        boot_vcpus (int):
        config (Any):
        hypervisor (Hypervisor):
        id (UUID):
        kvm_hyperv (bool):
        max_vcpus (int):
        memory_hugepages (bool):
        memory_mergeable (bool):
        memory_prefault (bool):
        memory_shared (bool):
        memory_size (int):
        memory_thp (bool):
        name (str):
        status (VmStatus):
        tags (list[str]):
        boot_source_id (None | Unset | UUID):
        cloud_init_meta_data (None | str | Unset):
        cloud_init_network_config (None | str | Unset):
        cloud_init_user_data (None | str | Unset):
        cpu_topology (Any | Unset):
        description (None | str | Unset):
        host_id (None | Unset | UUID):
        image_ref (None | str | Unset):
        memory_hotplug_size (int | None | Unset):
        memory_hugepage_size (int | None | Unset):
        placement_policy (None | PlacementPolicy | Unset):
    """

    boot_mode: BootMode
    boot_vcpus: int
    config: Any
    hypervisor: Hypervisor
    id: UUID
    kvm_hyperv: bool
    max_vcpus: int
    memory_hugepages: bool
    memory_mergeable: bool
    memory_prefault: bool
    memory_shared: bool
    memory_size: int
    memory_thp: bool
    name: str
    status: VmStatus
    tags: list[str]
    boot_source_id: None | Unset | UUID = UNSET
    cloud_init_meta_data: None | str | Unset = UNSET
    cloud_init_network_config: None | str | Unset = UNSET
    cloud_init_user_data: None | str | Unset = UNSET
    cpu_topology: Any | Unset = UNSET
    description: None | str | Unset = UNSET
    host_id: None | Unset | UUID = UNSET
    image_ref: None | str | Unset = UNSET
    memory_hotplug_size: int | None | Unset = UNSET
    memory_hugepage_size: int | None | Unset = UNSET
    placement_policy: None | PlacementPolicy | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.placement_policy import PlacementPolicy

        boot_mode = self.boot_mode.value

        boot_vcpus = self.boot_vcpus

        config = self.config

        hypervisor = self.hypervisor.value

        id = str(self.id)

        kvm_hyperv = self.kvm_hyperv

        max_vcpus = self.max_vcpus

        memory_hugepages = self.memory_hugepages

        memory_mergeable = self.memory_mergeable

        memory_prefault = self.memory_prefault

        memory_shared = self.memory_shared

        memory_size = self.memory_size

        memory_thp = self.memory_thp

        name = self.name

        status = self.status.value

        tags = self.tags

        boot_source_id: None | str | Unset
        if isinstance(self.boot_source_id, Unset):
            boot_source_id = UNSET
        elif isinstance(self.boot_source_id, UUID):
            boot_source_id = str(self.boot_source_id)
        else:
            boot_source_id = self.boot_source_id

        cloud_init_meta_data: None | str | Unset
        if isinstance(self.cloud_init_meta_data, Unset):
            cloud_init_meta_data = UNSET
        else:
            cloud_init_meta_data = self.cloud_init_meta_data

        cloud_init_network_config: None | str | Unset
        if isinstance(self.cloud_init_network_config, Unset):
            cloud_init_network_config = UNSET
        else:
            cloud_init_network_config = self.cloud_init_network_config

        cloud_init_user_data: None | str | Unset
        if isinstance(self.cloud_init_user_data, Unset):
            cloud_init_user_data = UNSET
        else:
            cloud_init_user_data = self.cloud_init_user_data

        cpu_topology = self.cpu_topology

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        host_id: None | str | Unset
        if isinstance(self.host_id, Unset):
            host_id = UNSET
        elif isinstance(self.host_id, UUID):
            host_id = str(self.host_id)
        else:
            host_id = self.host_id

        image_ref: None | str | Unset
        if isinstance(self.image_ref, Unset):
            image_ref = UNSET
        else:
            image_ref = self.image_ref

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

        placement_policy: dict[str, Any] | None | Unset
        if isinstance(self.placement_policy, Unset):
            placement_policy = UNSET
        elif isinstance(self.placement_policy, PlacementPolicy):
            placement_policy = self.placement_policy.to_dict()
        else:
            placement_policy = self.placement_policy

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "boot_mode": boot_mode,
                "boot_vcpus": boot_vcpus,
                "config": config,
                "hypervisor": hypervisor,
                "id": id,
                "kvm_hyperv": kvm_hyperv,
                "max_vcpus": max_vcpus,
                "memory_hugepages": memory_hugepages,
                "memory_mergeable": memory_mergeable,
                "memory_prefault": memory_prefault,
                "memory_shared": memory_shared,
                "memory_size": memory_size,
                "memory_thp": memory_thp,
                "name": name,
                "status": status,
                "tags": tags,
            }
        )
        if boot_source_id is not UNSET:
            field_dict["boot_source_id"] = boot_source_id
        if cloud_init_meta_data is not UNSET:
            field_dict["cloud_init_meta_data"] = cloud_init_meta_data
        if cloud_init_network_config is not UNSET:
            field_dict["cloud_init_network_config"] = cloud_init_network_config
        if cloud_init_user_data is not UNSET:
            field_dict["cloud_init_user_data"] = cloud_init_user_data
        if cpu_topology is not UNSET:
            field_dict["cpu_topology"] = cpu_topology
        if description is not UNSET:
            field_dict["description"] = description
        if host_id is not UNSET:
            field_dict["host_id"] = host_id
        if image_ref is not UNSET:
            field_dict["image_ref"] = image_ref
        if memory_hotplug_size is not UNSET:
            field_dict["memory_hotplug_size"] = memory_hotplug_size
        if memory_hugepage_size is not UNSET:
            field_dict["memory_hugepage_size"] = memory_hugepage_size
        if placement_policy is not UNSET:
            field_dict["placement_policy"] = placement_policy

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        from ..models.placement_policy import PlacementPolicy

        d = dict(src_dict)
        boot_mode = BootMode(d.pop("boot_mode"))

        boot_vcpus = d.pop("boot_vcpus")

        config = d.pop("config")

        hypervisor = Hypervisor(d.pop("hypervisor"))

        id = UUID(d.pop("id"))

        kvm_hyperv = d.pop("kvm_hyperv")

        max_vcpus = d.pop("max_vcpus")

        memory_hugepages = d.pop("memory_hugepages")

        memory_mergeable = d.pop("memory_mergeable")

        memory_prefault = d.pop("memory_prefault")

        memory_shared = d.pop("memory_shared")

        memory_size = d.pop("memory_size")

        memory_thp = d.pop("memory_thp")

        name = d.pop("name")

        status = VmStatus(d.pop("status"))

        tags = cast(list[str], d.pop("tags"))

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

        def _parse_cloud_init_meta_data(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cloud_init_meta_data = _parse_cloud_init_meta_data(d.pop("cloud_init_meta_data", UNSET))

        def _parse_cloud_init_network_config(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cloud_init_network_config = _parse_cloud_init_network_config(d.pop("cloud_init_network_config", UNSET))

        def _parse_cloud_init_user_data(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cloud_init_user_data = _parse_cloud_init_user_data(d.pop("cloud_init_user_data", UNSET))

        cpu_topology = d.pop("cpu_topology", UNSET)

        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))

        def _parse_host_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                host_id_type_0 = UUID(data)

                return host_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        host_id = _parse_host_id(d.pop("host_id", UNSET))

        def _parse_image_ref(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        image_ref = _parse_image_ref(d.pop("image_ref", UNSET))

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

        def _parse_placement_policy(data: object) -> None | PlacementPolicy | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                placement_policy_type_1 = PlacementPolicy.from_dict(data)

                return placement_policy_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | PlacementPolicy | Unset, data)

        placement_policy = _parse_placement_policy(d.pop("placement_policy", UNSET))

        vm = cls(
            boot_mode=boot_mode,
            boot_vcpus=boot_vcpus,
            config=config,
            hypervisor=hypervisor,
            id=id,
            kvm_hyperv=kvm_hyperv,
            max_vcpus=max_vcpus,
            memory_hugepages=memory_hugepages,
            memory_mergeable=memory_mergeable,
            memory_prefault=memory_prefault,
            memory_shared=memory_shared,
            memory_size=memory_size,
            memory_thp=memory_thp,
            name=name,
            status=status,
            tags=tags,
            boot_source_id=boot_source_id,
            cloud_init_meta_data=cloud_init_meta_data,
            cloud_init_network_config=cloud_init_network_config,
            cloud_init_user_data=cloud_init_user_data,
            cpu_topology=cpu_topology,
            description=description,
            host_id=host_id,
            image_ref=image_ref,
            memory_hotplug_size=memory_hotplug_size,
            memory_hugepage_size=memory_hugepage_size,
            placement_policy=placement_policy,
        )

        vm.additional_properties = d
        return vm

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
