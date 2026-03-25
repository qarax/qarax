from __future__ import annotations

import datetime
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from dateutil.parser import isoparse

from ..models.host_status import HostStatus
from ..types import UNSET, Unset

T = TypeVar("T", bound="Host")


@_attrs_define
class Host:
    """
    Attributes:
        address (str):
        host_user (str):
        id (UUID):
        name (str):
        port (int):
        status (HostStatus):
        update_available (bool): True when `node_version` differs from the control-plane version.
        available_memory_bytes (int | None | Unset):
        cloud_hypervisor_version (None | str | Unset):
        disk_available_bytes (int | None | Unset):
        disk_total_bytes (int | None | Unset):
        kernel_version (None | str | Unset):
        last_deployed_image (None | str | Unset): Last bootc image deployed to this host via the `/deploy` endpoint.
        load_average (float | None | Unset):
        node_version (None | str | Unset): Version of the qarax-node agent running on this host.
        resources_updated_at (datetime.datetime | None | Unset):
        total_cpus (int | None | Unset):
        total_memory_bytes (int | None | Unset):
    """

    address: str
    host_user: str
    id: UUID
    name: str
    port: int
    status: HostStatus
    update_available: bool
    available_memory_bytes: int | None | Unset = UNSET
    cloud_hypervisor_version: None | str | Unset = UNSET
    disk_available_bytes: int | None | Unset = UNSET
    disk_total_bytes: int | None | Unset = UNSET
    kernel_version: None | str | Unset = UNSET
    last_deployed_image: None | str | Unset = UNSET
    load_average: float | None | Unset = UNSET
    node_version: None | str | Unset = UNSET
    resources_updated_at: datetime.datetime | None | Unset = UNSET
    total_cpus: int | None | Unset = UNSET
    total_memory_bytes: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        address = self.address

        host_user = self.host_user

        id = str(self.id)

        name = self.name

        port = self.port

        status = self.status.value

        update_available = self.update_available

        available_memory_bytes: int | None | Unset
        if isinstance(self.available_memory_bytes, Unset):
            available_memory_bytes = UNSET
        else:
            available_memory_bytes = self.available_memory_bytes

        cloud_hypervisor_version: None | str | Unset
        if isinstance(self.cloud_hypervisor_version, Unset):
            cloud_hypervisor_version = UNSET
        else:
            cloud_hypervisor_version = self.cloud_hypervisor_version

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

        kernel_version: None | str | Unset
        if isinstance(self.kernel_version, Unset):
            kernel_version = UNSET
        else:
            kernel_version = self.kernel_version

        last_deployed_image: None | str | Unset
        if isinstance(self.last_deployed_image, Unset):
            last_deployed_image = UNSET
        else:
            last_deployed_image = self.last_deployed_image

        load_average: float | None | Unset
        if isinstance(self.load_average, Unset):
            load_average = UNSET
        else:
            load_average = self.load_average

        node_version: None | str | Unset
        if isinstance(self.node_version, Unset):
            node_version = UNSET
        else:
            node_version = self.node_version

        resources_updated_at: None | str | Unset
        if isinstance(self.resources_updated_at, Unset):
            resources_updated_at = UNSET
        elif isinstance(self.resources_updated_at, datetime.datetime):
            resources_updated_at = self.resources_updated_at.isoformat()
        else:
            resources_updated_at = self.resources_updated_at

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
                "address": address,
                "host_user": host_user,
                "id": id,
                "name": name,
                "port": port,
                "status": status,
                "update_available": update_available,
            }
        )
        if available_memory_bytes is not UNSET:
            field_dict["available_memory_bytes"] = available_memory_bytes
        if cloud_hypervisor_version is not UNSET:
            field_dict["cloud_hypervisor_version"] = cloud_hypervisor_version
        if disk_available_bytes is not UNSET:
            field_dict["disk_available_bytes"] = disk_available_bytes
        if disk_total_bytes is not UNSET:
            field_dict["disk_total_bytes"] = disk_total_bytes
        if kernel_version is not UNSET:
            field_dict["kernel_version"] = kernel_version
        if last_deployed_image is not UNSET:
            field_dict["last_deployed_image"] = last_deployed_image
        if load_average is not UNSET:
            field_dict["load_average"] = load_average
        if node_version is not UNSET:
            field_dict["node_version"] = node_version
        if resources_updated_at is not UNSET:
            field_dict["resources_updated_at"] = resources_updated_at
        if total_cpus is not UNSET:
            field_dict["total_cpus"] = total_cpus
        if total_memory_bytes is not UNSET:
            field_dict["total_memory_bytes"] = total_memory_bytes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        address = d.pop("address")

        host_user = d.pop("host_user")

        id = UUID(d.pop("id"))

        name = d.pop("name")

        port = d.pop("port")

        status = HostStatus(d.pop("status"))

        update_available = d.pop("update_available")

        def _parse_available_memory_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        available_memory_bytes = _parse_available_memory_bytes(d.pop("available_memory_bytes", UNSET))

        def _parse_cloud_hypervisor_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cloud_hypervisor_version = _parse_cloud_hypervisor_version(d.pop("cloud_hypervisor_version", UNSET))

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

        def _parse_kernel_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        kernel_version = _parse_kernel_version(d.pop("kernel_version", UNSET))

        def _parse_last_deployed_image(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        last_deployed_image = _parse_last_deployed_image(d.pop("last_deployed_image", UNSET))

        def _parse_load_average(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        load_average = _parse_load_average(d.pop("load_average", UNSET))

        def _parse_node_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        node_version = _parse_node_version(d.pop("node_version", UNSET))

        def _parse_resources_updated_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                resources_updated_at_type_0 = isoparse(data)

                return resources_updated_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        resources_updated_at = _parse_resources_updated_at(d.pop("resources_updated_at", UNSET))

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

        host = cls(
            address=address,
            host_user=host_user,
            id=id,
            name=name,
            port=port,
            status=status,
            update_available=update_available,
            available_memory_bytes=available_memory_bytes,
            cloud_hypervisor_version=cloud_hypervisor_version,
            disk_available_bytes=disk_available_bytes,
            disk_total_bytes=disk_total_bytes,
            kernel_version=kernel_version,
            last_deployed_image=last_deployed_image,
            load_average=load_average,
            node_version=node_version,
            resources_updated_at=resources_updated_at,
            total_cpus=total_cpus,
            total_memory_bytes=total_memory_bytes,
        )

        host.additional_properties = d
        return host

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
