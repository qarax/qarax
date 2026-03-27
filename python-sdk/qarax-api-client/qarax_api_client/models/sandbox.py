from __future__ import annotations

import datetime
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from dateutil.parser import isoparse

from ..models.sandbox_status import SandboxStatus
from ..models.vm_status import VmStatus
from ..types import UNSET, Unset

T = TypeVar("T", bound="Sandbox")


@_attrs_define
class Sandbox:
    """
    Attributes:
        created_at (datetime.datetime):
        id (UUID):
        idle_timeout_secs (int):
        last_activity_at (datetime.datetime):
        name (str):
        status (SandboxStatus):
        vm_id (UUID):
        error_message (None | str | Unset):
        ip_address (None | str | Unset):
        vm_status (None | Unset | VmStatus):
        vm_template_id (None | Unset | UUID):
    """

    created_at: datetime.datetime
    id: UUID
    idle_timeout_secs: int
    last_activity_at: datetime.datetime
    name: str
    status: SandboxStatus
    vm_id: UUID
    error_message: None | str | Unset = UNSET
    ip_address: None | str | Unset = UNSET
    vm_status: None | Unset | VmStatus = UNSET
    vm_template_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at.isoformat()

        id = str(self.id)

        idle_timeout_secs = self.idle_timeout_secs

        last_activity_at = self.last_activity_at.isoformat()

        name = self.name

        status = self.status.value

        vm_id = str(self.vm_id)

        error_message: None | str | Unset
        if isinstance(self.error_message, Unset):
            error_message = UNSET
        else:
            error_message = self.error_message

        ip_address: None | str | Unset
        if isinstance(self.ip_address, Unset):
            ip_address = UNSET
        else:
            ip_address = self.ip_address

        vm_status: None | str | Unset
        if isinstance(self.vm_status, Unset):
            vm_status = UNSET
        elif isinstance(self.vm_status, VmStatus):
            vm_status = self.vm_status.value
        else:
            vm_status = self.vm_status

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
                "created_at": created_at,
                "id": id,
                "idle_timeout_secs": idle_timeout_secs,
                "last_activity_at": last_activity_at,
                "name": name,
                "status": status,
                "vm_id": vm_id,
            }
        )
        if error_message is not UNSET:
            field_dict["error_message"] = error_message
        if ip_address is not UNSET:
            field_dict["ip_address"] = ip_address
        if vm_status is not UNSET:
            field_dict["vm_status"] = vm_status
        if vm_template_id is not UNSET:
            field_dict["vm_template_id"] = vm_template_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        created_at = isoparse(d.pop("created_at"))

        id = UUID(d.pop("id"))

        idle_timeout_secs = d.pop("idle_timeout_secs")

        last_activity_at = isoparse(d.pop("last_activity_at"))

        name = d.pop("name")

        status = SandboxStatus(d.pop("status"))

        vm_id = UUID(d.pop("vm_id"))

        def _parse_error_message(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error_message = _parse_error_message(d.pop("error_message", UNSET))

        def _parse_ip_address(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ip_address = _parse_ip_address(d.pop("ip_address", UNSET))

        def _parse_vm_status(data: object) -> None | Unset | VmStatus:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                vm_status_type_1 = VmStatus(data)

                return vm_status_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | VmStatus, data)

        vm_status = _parse_vm_status(d.pop("vm_status", UNSET))

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

        sandbox = cls(
            created_at=created_at,
            id=id,
            idle_timeout_secs=idle_timeout_secs,
            last_activity_at=last_activity_at,
            name=name,
            status=status,
            vm_id=vm_id,
            error_message=error_message,
            ip_address=ip_address,
            vm_status=vm_status,
            vm_template_id=vm_template_id,
        )

        sandbox.additional_properties = d
        return sandbox

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
