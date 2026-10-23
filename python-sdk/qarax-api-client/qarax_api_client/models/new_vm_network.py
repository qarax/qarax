from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.interface_type import InterfaceType
from ..models.vhost_mode import VhostMode
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.rate_limiter_config import RateLimiterConfig


T = TypeVar("T", bound="NewVmNetwork")


@_attrs_define
class NewVmNetwork:
    """Network interface config for create-VM request. Passed to qarax-node; id is required.

    Attributes:
        id (str): Unique device id (e.g. "net0")
        host_mac (None | str | Unset): Host-side MAC address
        interface_type (InterfaceType | None | Unset):
        iommu (bool | None | Unset): Enable IOMMU for the device
        ip (None | str | Unset): IPv4 or IPv6 address (optional)
        mac (None | str | Unset): Guest MAC address (optional)
        mask (None | str | Unset): Network mask (optional)
        mtu (int | None | Unset): MTU (optional)
        network_id (None | Unset | UUID): Network ID for managed networking (IPAM)
        num_queues (int | None | Unset): Number of virtio queues
        offload_csum (bool | None | Unset): Enable checksum offload
        offload_tso (bool | None | Unset): Enable TCP Segmentation Offload
        offload_ufo (bool | None | Unset): Enable UDP Fragmentation Offload
        pci_segment (int | None | Unset): PCI segment number
        queue_size (int | None | Unset): Size of each queue
        rate_limiter (None | RateLimiterConfig | Unset):
        tap (None | str | Unset): Pre-created TAP device name (optional)
        vhost_mode (None | Unset | VhostMode):
        vhost_socket (None | str | Unset): Unix socket path for vhost-user backend
        vhost_user (bool | None | Unset): Enable vhost-user networking
    """

    id: str
    host_mac: None | str | Unset = UNSET
    interface_type: InterfaceType | None | Unset = UNSET
    iommu: bool | None | Unset = UNSET
    ip: None | str | Unset = UNSET
    mac: None | str | Unset = UNSET
    mask: None | str | Unset = UNSET
    mtu: int | None | Unset = UNSET
    network_id: None | Unset | UUID = UNSET
    num_queues: int | None | Unset = UNSET
    offload_csum: bool | None | Unset = UNSET
    offload_tso: bool | None | Unset = UNSET
    offload_ufo: bool | None | Unset = UNSET
    pci_segment: int | None | Unset = UNSET
    queue_size: int | None | Unset = UNSET
    rate_limiter: None | RateLimiterConfig | Unset = UNSET
    tap: None | str | Unset = UNSET
    vhost_mode: None | Unset | VhostMode = UNSET
    vhost_socket: None | str | Unset = UNSET
    vhost_user: bool | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.rate_limiter_config import RateLimiterConfig

        id = self.id

        host_mac: None | str | Unset
        if isinstance(self.host_mac, Unset):
            host_mac = UNSET
        else:
            host_mac = self.host_mac

        interface_type: None | str | Unset
        if isinstance(self.interface_type, Unset):
            interface_type = UNSET
        elif isinstance(self.interface_type, InterfaceType):
            interface_type = self.interface_type.value
        else:
            interface_type = self.interface_type

        iommu: bool | None | Unset
        if isinstance(self.iommu, Unset):
            iommu = UNSET
        else:
            iommu = self.iommu

        ip: None | str | Unset
        if isinstance(self.ip, Unset):
            ip = UNSET
        else:
            ip = self.ip

        mac: None | str | Unset
        if isinstance(self.mac, Unset):
            mac = UNSET
        else:
            mac = self.mac

        mask: None | str | Unset
        if isinstance(self.mask, Unset):
            mask = UNSET
        else:
            mask = self.mask

        mtu: int | None | Unset
        if isinstance(self.mtu, Unset):
            mtu = UNSET
        else:
            mtu = self.mtu

        network_id: None | str | Unset
        if isinstance(self.network_id, Unset):
            network_id = UNSET
        elif isinstance(self.network_id, UUID):
            network_id = str(self.network_id)
        else:
            network_id = self.network_id

        num_queues: int | None | Unset
        if isinstance(self.num_queues, Unset):
            num_queues = UNSET
        else:
            num_queues = self.num_queues

        offload_csum: bool | None | Unset
        if isinstance(self.offload_csum, Unset):
            offload_csum = UNSET
        else:
            offload_csum = self.offload_csum

        offload_tso: bool | None | Unset
        if isinstance(self.offload_tso, Unset):
            offload_tso = UNSET
        else:
            offload_tso = self.offload_tso

        offload_ufo: bool | None | Unset
        if isinstance(self.offload_ufo, Unset):
            offload_ufo = UNSET
        else:
            offload_ufo = self.offload_ufo

        pci_segment: int | None | Unset
        if isinstance(self.pci_segment, Unset):
            pci_segment = UNSET
        else:
            pci_segment = self.pci_segment

        queue_size: int | None | Unset
        if isinstance(self.queue_size, Unset):
            queue_size = UNSET
        else:
            queue_size = self.queue_size

        rate_limiter: dict[str, Any] | None | Unset
        if isinstance(self.rate_limiter, Unset):
            rate_limiter = UNSET
        elif isinstance(self.rate_limiter, RateLimiterConfig):
            rate_limiter = self.rate_limiter.to_dict()
        else:
            rate_limiter = self.rate_limiter

        tap: None | str | Unset
        if isinstance(self.tap, Unset):
            tap = UNSET
        else:
            tap = self.tap

        vhost_mode: None | str | Unset
        if isinstance(self.vhost_mode, Unset):
            vhost_mode = UNSET
        elif isinstance(self.vhost_mode, VhostMode):
            vhost_mode = self.vhost_mode.value
        else:
            vhost_mode = self.vhost_mode

        vhost_socket: None | str | Unset
        if isinstance(self.vhost_socket, Unset):
            vhost_socket = UNSET
        else:
            vhost_socket = self.vhost_socket

        vhost_user: bool | None | Unset
        if isinstance(self.vhost_user, Unset):
            vhost_user = UNSET
        else:
            vhost_user = self.vhost_user

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
            }
        )
        if host_mac is not UNSET:
            field_dict["host_mac"] = host_mac
        if interface_type is not UNSET:
            field_dict["interface_type"] = interface_type
        if iommu is not UNSET:
            field_dict["iommu"] = iommu
        if ip is not UNSET:
            field_dict["ip"] = ip
        if mac is not UNSET:
            field_dict["mac"] = mac
        if mask is not UNSET:
            field_dict["mask"] = mask
        if mtu is not UNSET:
            field_dict["mtu"] = mtu
        if network_id is not UNSET:
            field_dict["network_id"] = network_id
        if num_queues is not UNSET:
            field_dict["num_queues"] = num_queues
        if offload_csum is not UNSET:
            field_dict["offload_csum"] = offload_csum
        if offload_tso is not UNSET:
            field_dict["offload_tso"] = offload_tso
        if offload_ufo is not UNSET:
            field_dict["offload_ufo"] = offload_ufo
        if pci_segment is not UNSET:
            field_dict["pci_segment"] = pci_segment
        if queue_size is not UNSET:
            field_dict["queue_size"] = queue_size
        if rate_limiter is not UNSET:
            field_dict["rate_limiter"] = rate_limiter
        if tap is not UNSET:
            field_dict["tap"] = tap
        if vhost_mode is not UNSET:
            field_dict["vhost_mode"] = vhost_mode
        if vhost_socket is not UNSET:
            field_dict["vhost_socket"] = vhost_socket
        if vhost_user is not UNSET:
            field_dict["vhost_user"] = vhost_user

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        from ..models.rate_limiter_config import RateLimiterConfig

        d = dict(src_dict)
        id = d.pop("id")

        def _parse_host_mac(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        host_mac = _parse_host_mac(d.pop("host_mac", UNSET))

        def _parse_interface_type(data: object) -> InterfaceType | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                interface_type_type_0 = InterfaceType(data)

                return interface_type_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(InterfaceType | None | Unset, data)

        interface_type = _parse_interface_type(d.pop("interface_type", UNSET))

        def _parse_iommu(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        iommu = _parse_iommu(d.pop("iommu", UNSET))

        def _parse_ip(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ip = _parse_ip(d.pop("ip", UNSET))

        def _parse_mac(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mac = _parse_mac(d.pop("mac", UNSET))

        def _parse_mask(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mask = _parse_mask(d.pop("mask", UNSET))

        def _parse_mtu(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        mtu = _parse_mtu(d.pop("mtu", UNSET))

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

        def _parse_num_queues(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        num_queues = _parse_num_queues(d.pop("num_queues", UNSET))

        def _parse_offload_csum(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        offload_csum = _parse_offload_csum(d.pop("offload_csum", UNSET))

        def _parse_offload_tso(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        offload_tso = _parse_offload_tso(d.pop("offload_tso", UNSET))

        def _parse_offload_ufo(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        offload_ufo = _parse_offload_ufo(d.pop("offload_ufo", UNSET))

        def _parse_pci_segment(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        pci_segment = _parse_pci_segment(d.pop("pci_segment", UNSET))

        def _parse_queue_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        queue_size = _parse_queue_size(d.pop("queue_size", UNSET))

        def _parse_rate_limiter(data: object) -> None | RateLimiterConfig | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                rate_limiter_type_0 = RateLimiterConfig.from_dict(data)

                return rate_limiter_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | RateLimiterConfig | Unset, data)

        rate_limiter = _parse_rate_limiter(d.pop("rate_limiter", UNSET))

        def _parse_tap(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tap = _parse_tap(d.pop("tap", UNSET))

        def _parse_vhost_mode(data: object) -> None | Unset | VhostMode:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                vhost_mode_type_0 = VhostMode(data)

                return vhost_mode_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | VhostMode, data)

        vhost_mode = _parse_vhost_mode(d.pop("vhost_mode", UNSET))

        def _parse_vhost_socket(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        vhost_socket = _parse_vhost_socket(d.pop("vhost_socket", UNSET))

        def _parse_vhost_user(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        vhost_user = _parse_vhost_user(d.pop("vhost_user", UNSET))

        new_vm_network = cls(
            id=id,
            host_mac=host_mac,
            interface_type=interface_type,
            iommu=iommu,
            ip=ip,
            mac=mac,
            mask=mask,
            mtu=mtu,
            network_id=network_id,
            num_queues=num_queues,
            offload_csum=offload_csum,
            offload_tso=offload_tso,
            offload_ufo=offload_ufo,
            pci_segment=pci_segment,
            queue_size=queue_size,
            rate_limiter=rate_limiter,
            tap=tap,
            vhost_mode=vhost_mode,
            vhost_socket=vhost_socket,
            vhost_user=vhost_user,
        )

        new_vm_network.additional_properties = d
        return new_vm_network

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
