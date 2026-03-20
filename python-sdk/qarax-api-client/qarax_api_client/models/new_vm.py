from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.boot_mode import BootMode
from ..models.hypervisor import Hypervisor
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.new_vm_network import NewVmNetwork


T = TypeVar("T", bound="NewVm")


@_attrs_define
class NewVm:
    """
    Attributes:
        name (str):
        accelerator_config (Any | Unset): Accelerator (GPU) configuration. When set, GPU-aware scheduling picks a
            host with available GPUs matching these filters, and VFIO passthrough
            devices are attached to the VM.
        boot_mode (BootMode | None | Unset):
        boot_source_id (None | Unset | UUID):
        boot_vcpus (int | None | Unset):
        cloud_init_meta_data (None | str | Unset): Cloud-init meta-data (raw YAML). Auto-generated from vm id/name if
            omitted.
        cloud_init_network_config (None | str | Unset): Cloud-init network-config (raw YAML). When provided, kernel
            `ip=` cmdline
            params are suppressed so cloud-init owns networking.
        cloud_init_user_data (None | str | Unset): Cloud-init user-data (raw YAML). When provided a NoCloud seed image
            is
            generated and attached as a read-only disk to the VM.
        config (Any | Unset):
        cpu_topology (Any | Unset):
        description (None | str | Unset):
        hypervisor (Hypervisor | None | Unset):
        image_ref (None | str | Unset): OCI image reference to use as root filesystem (e.g.
            "docker.io/library/ubuntu:22.04").
            When set, the handler will check whether the selected host has an OverlayBD storage pool.
            If so, the image is served via lazy block loading (virtio-blk); otherwise via virtiofs.
        instance_type_id (None | Unset | UUID):
        kvm_hyperv (bool | None | Unset):
        max_vcpus (int | None | Unset):
        memory_hotplug_size (int | None | Unset):
        memory_hugepage_size (int | None | Unset):
        memory_hugepages (bool | None | Unset):
        memory_mergeable (bool | None | Unset):
        memory_prefault (bool | None | Unset):
        memory_shared (bool | None | Unset):
        memory_size (int | None | Unset):
        memory_thp (bool | None | Unset):
        network_id (None | Unset | UUID): Network ID to attach the VM to (triggers IPAM allocation).
        networks (list[NewVmNetwork] | None | Unset): Optional network interfaces to attach at create time (passed to
            qarax-node).
        root_disk_object_id (None | Unset | UUID):
        tags (list[str] | None | Unset):
        vm_template_id (None | Unset | UUID):
    """

    name: str
    accelerator_config: Any | Unset = UNSET
    boot_mode: BootMode | None | Unset = UNSET
    boot_source_id: None | Unset | UUID = UNSET
    boot_vcpus: int | None | Unset = UNSET
    cloud_init_meta_data: None | str | Unset = UNSET
    cloud_init_network_config: None | str | Unset = UNSET
    cloud_init_user_data: None | str | Unset = UNSET
    config: Any | Unset = UNSET
    cpu_topology: Any | Unset = UNSET
    description: None | str | Unset = UNSET
    hypervisor: Hypervisor | None | Unset = UNSET
    image_ref: None | str | Unset = UNSET
    instance_type_id: None | Unset | UUID = UNSET
    kvm_hyperv: bool | None | Unset = UNSET
    max_vcpus: int | None | Unset = UNSET
    memory_hotplug_size: int | None | Unset = UNSET
    memory_hugepage_size: int | None | Unset = UNSET
    memory_hugepages: bool | None | Unset = UNSET
    memory_mergeable: bool | None | Unset = UNSET
    memory_prefault: bool | None | Unset = UNSET
    memory_shared: bool | None | Unset = UNSET
    memory_size: int | None | Unset = UNSET
    memory_thp: bool | None | Unset = UNSET
    network_id: None | Unset | UUID = UNSET
    networks: list[NewVmNetwork] | None | Unset = UNSET
    root_disk_object_id: None | Unset | UUID = UNSET
    tags: list[str] | None | Unset = UNSET
    vm_template_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        accelerator_config = self.accelerator_config

        boot_mode: None | str | Unset
        if isinstance(self.boot_mode, Unset):
            boot_mode = UNSET
        elif isinstance(self.boot_mode, BootMode):
            boot_mode = self.boot_mode.value
        else:
            boot_mode = self.boot_mode

        boot_source_id: None | str | Unset
        if isinstance(self.boot_source_id, Unset):
            boot_source_id = UNSET
        elif isinstance(self.boot_source_id, UUID):
            boot_source_id = str(self.boot_source_id)
        else:
            boot_source_id = self.boot_source_id

        boot_vcpus: int | None | Unset
        if isinstance(self.boot_vcpus, Unset):
            boot_vcpus = UNSET
        else:
            boot_vcpus = self.boot_vcpus

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

        config = self.config

        cpu_topology = self.cpu_topology

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        hypervisor: None | str | Unset
        if isinstance(self.hypervisor, Unset):
            hypervisor = UNSET
        elif isinstance(self.hypervisor, Hypervisor):
            hypervisor = self.hypervisor.value
        else:
            hypervisor = self.hypervisor

        image_ref: None | str | Unset
        if isinstance(self.image_ref, Unset):
            image_ref = UNSET
        else:
            image_ref = self.image_ref

        instance_type_id: None | str | Unset
        if isinstance(self.instance_type_id, Unset):
            instance_type_id = UNSET
        elif isinstance(self.instance_type_id, UUID):
            instance_type_id = str(self.instance_type_id)
        else:
            instance_type_id = self.instance_type_id

        kvm_hyperv: bool | None | Unset
        if isinstance(self.kvm_hyperv, Unset):
            kvm_hyperv = UNSET
        else:
            kvm_hyperv = self.kvm_hyperv

        max_vcpus: int | None | Unset
        if isinstance(self.max_vcpus, Unset):
            max_vcpus = UNSET
        else:
            max_vcpus = self.max_vcpus

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

        memory_size: int | None | Unset
        if isinstance(self.memory_size, Unset):
            memory_size = UNSET
        else:
            memory_size = self.memory_size

        memory_thp: bool | None | Unset
        if isinstance(self.memory_thp, Unset):
            memory_thp = UNSET
        else:
            memory_thp = self.memory_thp

        network_id: None | str | Unset
        if isinstance(self.network_id, Unset):
            network_id = UNSET
        elif isinstance(self.network_id, UUID):
            network_id = str(self.network_id)
        else:
            network_id = self.network_id

        networks: list[dict[str, Any]] | None | Unset
        if isinstance(self.networks, Unset):
            networks = UNSET
        elif isinstance(self.networks, list):
            networks = []
            for networks_type_0_item_data in self.networks:
                networks_type_0_item = networks_type_0_item_data.to_dict()
                networks.append(networks_type_0_item)

        else:
            networks = self.networks

        root_disk_object_id: None | str | Unset
        if isinstance(self.root_disk_object_id, Unset):
            root_disk_object_id = UNSET
        elif isinstance(self.root_disk_object_id, UUID):
            root_disk_object_id = str(self.root_disk_object_id)
        else:
            root_disk_object_id = self.root_disk_object_id

        tags: list[str] | None | Unset
        if isinstance(self.tags, Unset):
            tags = UNSET
        elif isinstance(self.tags, list):
            tags = self.tags

        else:
            tags = self.tags

        vm_template_id: None | str | Unset
        if isinstance(self.vm_template_id, Unset):
            vm_template_id = UNSET
        elif isinstance(self.vm_template_id, UUID):
            vm_template_id = str(self.vm_template_id)
        else:
            vm_template_id = self.vm_template_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
            }
        )
        if accelerator_config is not UNSET:
            field_dict["accelerator_config"] = accelerator_config
        if boot_mode is not UNSET:
            field_dict["boot_mode"] = boot_mode
        if boot_source_id is not UNSET:
            field_dict["boot_source_id"] = boot_source_id
        if boot_vcpus is not UNSET:
            field_dict["boot_vcpus"] = boot_vcpus
        if cloud_init_meta_data is not UNSET:
            field_dict["cloud_init_meta_data"] = cloud_init_meta_data
        if cloud_init_network_config is not UNSET:
            field_dict["cloud_init_network_config"] = cloud_init_network_config
        if cloud_init_user_data is not UNSET:
            field_dict["cloud_init_user_data"] = cloud_init_user_data
        if config is not UNSET:
            field_dict["config"] = config
        if cpu_topology is not UNSET:
            field_dict["cpu_topology"] = cpu_topology
        if description is not UNSET:
            field_dict["description"] = description
        if hypervisor is not UNSET:
            field_dict["hypervisor"] = hypervisor
        if image_ref is not UNSET:
            field_dict["image_ref"] = image_ref
        if instance_type_id is not UNSET:
            field_dict["instance_type_id"] = instance_type_id
        if kvm_hyperv is not UNSET:
            field_dict["kvm_hyperv"] = kvm_hyperv
        if max_vcpus is not UNSET:
            field_dict["max_vcpus"] = max_vcpus
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
        if memory_size is not UNSET:
            field_dict["memory_size"] = memory_size
        if memory_thp is not UNSET:
            field_dict["memory_thp"] = memory_thp
        if network_id is not UNSET:
            field_dict["network_id"] = network_id
        if networks is not UNSET:
            field_dict["networks"] = networks
        if root_disk_object_id is not UNSET:
            field_dict["root_disk_object_id"] = root_disk_object_id
        if tags is not UNSET:
            field_dict["tags"] = tags
        if vm_template_id is not UNSET:
            field_dict["vm_template_id"] = vm_template_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        from ..models.new_vm_network import NewVmNetwork

        d = dict(src_dict)
        name = d.pop("name")

        accelerator_config = d.pop("accelerator_config", UNSET)

        def _parse_boot_mode(data: object) -> BootMode | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                boot_mode_type_1 = BootMode(data)

                return boot_mode_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BootMode | None | Unset, data)

        boot_mode = _parse_boot_mode(d.pop("boot_mode", UNSET))

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

        def _parse_boot_vcpus(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        boot_vcpus = _parse_boot_vcpus(d.pop("boot_vcpus", UNSET))

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

        config = d.pop("config", UNSET)

        cpu_topology = d.pop("cpu_topology", UNSET)

        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))

        def _parse_hypervisor(data: object) -> Hypervisor | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                hypervisor_type_1 = Hypervisor(data)

                return hypervisor_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Hypervisor | None | Unset, data)

        hypervisor = _parse_hypervisor(d.pop("hypervisor", UNSET))

        def _parse_image_ref(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        image_ref = _parse_image_ref(d.pop("image_ref", UNSET))

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

        def _parse_kvm_hyperv(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        kvm_hyperv = _parse_kvm_hyperv(d.pop("kvm_hyperv", UNSET))

        def _parse_max_vcpus(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        max_vcpus = _parse_max_vcpus(d.pop("max_vcpus", UNSET))

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

        def _parse_memory_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        memory_size = _parse_memory_size(d.pop("memory_size", UNSET))

        def _parse_memory_thp(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        memory_thp = _parse_memory_thp(d.pop("memory_thp", UNSET))

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

        def _parse_networks(data: object) -> list[NewVmNetwork] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                networks_type_0 = []
                _networks_type_0 = data
                for networks_type_0_item_data in _networks_type_0:
                    networks_type_0_item = NewVmNetwork.from_dict(networks_type_0_item_data)

                    networks_type_0.append(networks_type_0_item)

                return networks_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[NewVmNetwork] | None | Unset, data)

        networks = _parse_networks(d.pop("networks", UNSET))

        def _parse_root_disk_object_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                root_disk_object_id_type_0 = UUID(data)

                return root_disk_object_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        root_disk_object_id = _parse_root_disk_object_id(d.pop("root_disk_object_id", UNSET))

        def _parse_tags(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                tags_type_0 = cast(list[str], data)

                return tags_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        tags = _parse_tags(d.pop("tags", UNSET))

        def _parse_vm_template_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                vm_template_id_type_0 = UUID(data)

                return vm_template_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        vm_template_id = _parse_vm_template_id(d.pop("vm_template_id", UNSET))

        new_vm = cls(
            name=name,
            accelerator_config=accelerator_config,
            boot_mode=boot_mode,
            boot_source_id=boot_source_id,
            boot_vcpus=boot_vcpus,
            cloud_init_meta_data=cloud_init_meta_data,
            cloud_init_network_config=cloud_init_network_config,
            cloud_init_user_data=cloud_init_user_data,
            config=config,
            cpu_topology=cpu_topology,
            description=description,
            hypervisor=hypervisor,
            image_ref=image_ref,
            instance_type_id=instance_type_id,
            kvm_hyperv=kvm_hyperv,
            max_vcpus=max_vcpus,
            memory_hotplug_size=memory_hotplug_size,
            memory_hugepage_size=memory_hugepage_size,
            memory_hugepages=memory_hugepages,
            memory_mergeable=memory_mergeable,
            memory_prefault=memory_prefault,
            memory_shared=memory_shared,
            memory_size=memory_size,
            memory_thp=memory_thp,
            network_id=network_id,
            networks=networks,
            root_disk_object_id=root_disk_object_id,
            tags=tags,
            vm_template_id=vm_template_id,
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
