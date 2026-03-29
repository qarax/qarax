from __future__ import annotations

from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="VmDisk")


@_attrs_define
class VmDisk:
    """
    Attributes:
        config (Any):
        device_path (str):
        direct (bool):
        id (UUID):
        logical_name (str):
        num_queues (int):
        pci_segment (int):
        queue_size (int):
        read_only (bool):
        vhost_user (bool):
        vm_id (UUID):
        boot_order (int | None | Unset):
        rate_limit_group (None | str | Unset):
        rate_limiter (Any | Unset):
        serial_number (None | str | Unset):
        storage_object_id (None | Unset | UUID):
        upper_storage_object_id (None | Unset | UUID): For OverlayBD disks: ID of the OverlaybdUpper StorageObject
            holding the
            persistent upper layer (upper.data + upper.index) on a Local or NFS pool.
            None = ephemeral (deleted on VM stop); Some = persistent across VM delete.
        vhost_socket (None | str | Unset):
    """

    config: Any
    device_path: str
    direct: bool
    id: UUID
    logical_name: str
    num_queues: int
    pci_segment: int
    queue_size: int
    read_only: bool
    vhost_user: bool
    vm_id: UUID
    boot_order: int | None | Unset = UNSET
    rate_limit_group: None | str | Unset = UNSET
    rate_limiter: Any | Unset = UNSET
    serial_number: None | str | Unset = UNSET
    storage_object_id: None | Unset | UUID = UNSET
    upper_storage_object_id: None | Unset | UUID = UNSET
    vhost_socket: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        config = self.config

        device_path = self.device_path

        direct = self.direct

        id = str(self.id)

        logical_name = self.logical_name

        num_queues = self.num_queues

        pci_segment = self.pci_segment

        queue_size = self.queue_size

        read_only = self.read_only

        vhost_user = self.vhost_user

        vm_id = str(self.vm_id)

        boot_order: int | None | Unset
        if isinstance(self.boot_order, Unset):
            boot_order = UNSET
        else:
            boot_order = self.boot_order

        rate_limit_group: None | str | Unset
        if isinstance(self.rate_limit_group, Unset):
            rate_limit_group = UNSET
        else:
            rate_limit_group = self.rate_limit_group

        rate_limiter = self.rate_limiter

        serial_number: None | str | Unset
        if isinstance(self.serial_number, Unset):
            serial_number = UNSET
        else:
            serial_number = self.serial_number

        storage_object_id: None | str | Unset
        if isinstance(self.storage_object_id, Unset):
            storage_object_id = UNSET
        elif isinstance(self.storage_object_id, UUID):
            storage_object_id = str(self.storage_object_id)
        else:
            storage_object_id = self.storage_object_id

        upper_storage_object_id: None | str | Unset
        if isinstance(self.upper_storage_object_id, Unset):
            upper_storage_object_id = UNSET
        elif isinstance(self.upper_storage_object_id, UUID):
            upper_storage_object_id = str(self.upper_storage_object_id)
        else:
            upper_storage_object_id = self.upper_storage_object_id

        vhost_socket: None | str | Unset
        if isinstance(self.vhost_socket, Unset):
            vhost_socket = UNSET
        else:
            vhost_socket = self.vhost_socket

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "config": config,
                "device_path": device_path,
                "direct": direct,
                "id": id,
                "logical_name": logical_name,
                "num_queues": num_queues,
                "pci_segment": pci_segment,
                "queue_size": queue_size,
                "read_only": read_only,
                "vhost_user": vhost_user,
                "vm_id": vm_id,
            }
        )
        if boot_order is not UNSET:
            field_dict["boot_order"] = boot_order
        if rate_limit_group is not UNSET:
            field_dict["rate_limit_group"] = rate_limit_group
        if rate_limiter is not UNSET:
            field_dict["rate_limiter"] = rate_limiter
        if serial_number is not UNSET:
            field_dict["serial_number"] = serial_number
        if storage_object_id is not UNSET:
            field_dict["storage_object_id"] = storage_object_id
        if upper_storage_object_id is not UNSET:
            field_dict["upper_storage_object_id"] = upper_storage_object_id
        if vhost_socket is not UNSET:
            field_dict["vhost_socket"] = vhost_socket

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        config = d.pop("config")

        device_path = d.pop("device_path")

        direct = d.pop("direct")

        id = UUID(d.pop("id"))

        logical_name = d.pop("logical_name")

        num_queues = d.pop("num_queues")

        pci_segment = d.pop("pci_segment")

        queue_size = d.pop("queue_size")

        read_only = d.pop("read_only")

        vhost_user = d.pop("vhost_user")

        vm_id = UUID(d.pop("vm_id"))

        def _parse_boot_order(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        boot_order = _parse_boot_order(d.pop("boot_order", UNSET))

        def _parse_rate_limit_group(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        rate_limit_group = _parse_rate_limit_group(d.pop("rate_limit_group", UNSET))

        rate_limiter = d.pop("rate_limiter", UNSET)

        def _parse_serial_number(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        serial_number = _parse_serial_number(d.pop("serial_number", UNSET))

        def _parse_storage_object_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                storage_object_id_type_0 = UUID(data)

                return storage_object_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        storage_object_id = _parse_storage_object_id(d.pop("storage_object_id", UNSET))

        def _parse_upper_storage_object_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                upper_storage_object_id_type_0 = UUID(data)

                return upper_storage_object_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        upper_storage_object_id = _parse_upper_storage_object_id(d.pop("upper_storage_object_id", UNSET))

        def _parse_vhost_socket(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        vhost_socket = _parse_vhost_socket(d.pop("vhost_socket", UNSET))

        vm_disk = cls(
            config=config,
            device_path=device_path,
            direct=direct,
            id=id,
            logical_name=logical_name,
            num_queues=num_queues,
            pci_segment=pci_segment,
            queue_size=queue_size,
            read_only=read_only,
            vhost_user=vhost_user,
            vm_id=vm_id,
            boot_order=boot_order,
            rate_limit_group=rate_limit_group,
            rate_limiter=rate_limiter,
            serial_number=serial_number,
            storage_object_id=storage_object_id,
            upper_storage_object_id=upper_storage_object_id,
            vhost_socket=vhost_socket,
        )

        vm_disk.additional_properties = d
        return vm_disk

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
