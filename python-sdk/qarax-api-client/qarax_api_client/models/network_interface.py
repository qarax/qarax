from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.interface_type import InterfaceType
from ..types import UNSET, Unset

T = TypeVar("T", bound="NetworkInterface")


@_attrs_define
class NetworkInterface:
    """
    Attributes:
        device_id (str):
        id (UUID):
        interface_type (InterfaceType):
        iommu (bool):
        mtu (int):
        num_queues (int):
        offload_csum (bool):
        offload_tso (bool):
        offload_ufo (bool):
        pci_segment (int):
        queue_size (int):
        vhost_user (bool):
        vm_id (UUID):
        host_mac (None | str | Unset):
        ip_address (None | str | Unset):
        mac_address (None | str | Unset):
        network_id (None | Unset | UUID):
        rate_limiter (Any | Unset):
        tap_name (None | str | Unset):
        vhost_mode (None | str | Unset):
        vhost_socket (None | str | Unset):
    """

    device_id: str
    id: UUID
    interface_type: InterfaceType
    iommu: bool
    mtu: int
    num_queues: int
    offload_csum: bool
    offload_tso: bool
    offload_ufo: bool
    pci_segment: int
    queue_size: int
    vhost_user: bool
    vm_id: UUID
    host_mac: None | str | Unset = UNSET
    ip_address: None | str | Unset = UNSET
    mac_address: None | str | Unset = UNSET
    network_id: None | Unset | UUID = UNSET
    rate_limiter: Any | Unset = UNSET
    tap_name: None | str | Unset = UNSET
    vhost_mode: None | str | Unset = UNSET
    vhost_socket: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        device_id = self.device_id

        id = str(self.id)

        interface_type = self.interface_type.value

        iommu = self.iommu

        mtu = self.mtu

        num_queues = self.num_queues

        offload_csum = self.offload_csum

        offload_tso = self.offload_tso

        offload_ufo = self.offload_ufo

        pci_segment = self.pci_segment

        queue_size = self.queue_size

        vhost_user = self.vhost_user

        vm_id = str(self.vm_id)

        host_mac: None | str | Unset
        if isinstance(self.host_mac, Unset):
            host_mac = UNSET
        else:
            host_mac = self.host_mac

        ip_address: None | str | Unset
        if isinstance(self.ip_address, Unset):
            ip_address = UNSET
        else:
            ip_address = self.ip_address

        mac_address: None | str | Unset
        if isinstance(self.mac_address, Unset):
            mac_address = UNSET
        else:
            mac_address = self.mac_address

        network_id: None | str | Unset
        if isinstance(self.network_id, Unset):
            network_id = UNSET
        elif isinstance(self.network_id, UUID):
            network_id = str(self.network_id)
        else:
            network_id = self.network_id

        rate_limiter = self.rate_limiter

        tap_name: None | str | Unset
        if isinstance(self.tap_name, Unset):
            tap_name = UNSET
        else:
            tap_name = self.tap_name

        vhost_mode: None | str | Unset
        if isinstance(self.vhost_mode, Unset):
            vhost_mode = UNSET
        else:
            vhost_mode = self.vhost_mode

        vhost_socket: None | str | Unset
        if isinstance(self.vhost_socket, Unset):
            vhost_socket = UNSET
        else:
            vhost_socket = self.vhost_socket

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "device_id": device_id,
                "id": id,
                "interface_type": interface_type,
                "iommu": iommu,
                "mtu": mtu,
                "num_queues": num_queues,
                "offload_csum": offload_csum,
                "offload_tso": offload_tso,
                "offload_ufo": offload_ufo,
                "pci_segment": pci_segment,
                "queue_size": queue_size,
                "vhost_user": vhost_user,
                "vm_id": vm_id,
            }
        )
        if host_mac is not UNSET:
            field_dict["host_mac"] = host_mac
        if ip_address is not UNSET:
            field_dict["ip_address"] = ip_address
        if mac_address is not UNSET:
            field_dict["mac_address"] = mac_address
        if network_id is not UNSET:
            field_dict["network_id"] = network_id
        if rate_limiter is not UNSET:
            field_dict["rate_limiter"] = rate_limiter
        if tap_name is not UNSET:
            field_dict["tap_name"] = tap_name
        if vhost_mode is not UNSET:
            field_dict["vhost_mode"] = vhost_mode
        if vhost_socket is not UNSET:
            field_dict["vhost_socket"] = vhost_socket

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        device_id = d.pop("device_id")

        id = UUID(d.pop("id"))

        interface_type = InterfaceType(d.pop("interface_type"))

        iommu = d.pop("iommu")

        mtu = d.pop("mtu")

        num_queues = d.pop("num_queues")

        offload_csum = d.pop("offload_csum")

        offload_tso = d.pop("offload_tso")

        offload_ufo = d.pop("offload_ufo")

        pci_segment = d.pop("pci_segment")

        queue_size = d.pop("queue_size")

        vhost_user = d.pop("vhost_user")

        vm_id = UUID(d.pop("vm_id"))

        def _parse_host_mac(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        host_mac = _parse_host_mac(d.pop("host_mac", UNSET))

        def _parse_ip_address(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ip_address = _parse_ip_address(d.pop("ip_address", UNSET))

        def _parse_mac_address(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mac_address = _parse_mac_address(d.pop("mac_address", UNSET))

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

        rate_limiter = d.pop("rate_limiter", UNSET)

        def _parse_tap_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tap_name = _parse_tap_name(d.pop("tap_name", UNSET))

        def _parse_vhost_mode(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        vhost_mode = _parse_vhost_mode(d.pop("vhost_mode", UNSET))

        def _parse_vhost_socket(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        vhost_socket = _parse_vhost_socket(d.pop("vhost_socket", UNSET))

        network_interface = cls(
            device_id=device_id,
            id=id,
            interface_type=interface_type,
            iommu=iommu,
            mtu=mtu,
            num_queues=num_queues,
            offload_csum=offload_csum,
            offload_tso=offload_tso,
            offload_ufo=offload_ufo,
            pci_segment=pci_segment,
            queue_size=queue_size,
            vhost_user=vhost_user,
            vm_id=vm_id,
            host_mac=host_mac,
            ip_address=ip_address,
            mac_address=mac_address,
            network_id=network_id,
            rate_limiter=rate_limiter,
            tap_name=tap_name,
            vhost_mode=vhost_mode,
            vhost_socket=vhost_socket,
        )

        network_interface.additional_properties = d
        return network_interface

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
