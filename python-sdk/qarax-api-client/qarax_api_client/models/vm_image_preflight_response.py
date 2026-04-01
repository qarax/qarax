from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.vm_image_preflight_check import VmImagePreflightCheck


T = TypeVar("T", bound="VmImagePreflightResponse")


@_attrs_define
class VmImagePreflightResponse:
    """
    Attributes:
        architecture (str):
        backend (str):
        bootable (bool):
        checks (list[VmImagePreflightCheck]):
        host_id (UUID):
        host_name (str):
        resolved_image_ref (str):
    """

    architecture: str
    backend: str
    bootable: bool
    checks: list[VmImagePreflightCheck]
    host_id: UUID
    host_name: str
    resolved_image_ref: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        architecture = self.architecture

        backend = self.backend

        bootable = self.bootable

        checks = []
        for checks_item_data in self.checks:
            checks_item = checks_item_data.to_dict()
            checks.append(checks_item)

        host_id = str(self.host_id)

        host_name = self.host_name

        resolved_image_ref = self.resolved_image_ref

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "architecture": architecture,
                "backend": backend,
                "bootable": bootable,
                "checks": checks,
                "host_id": host_id,
                "host_name": host_name,
                "resolved_image_ref": resolved_image_ref,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        from ..models.vm_image_preflight_check import VmImagePreflightCheck

        d = dict(src_dict)
        architecture = d.pop("architecture")

        backend = d.pop("backend")

        bootable = d.pop("bootable")

        checks = []
        _checks = d.pop("checks")
        for checks_item_data in _checks:
            checks_item = VmImagePreflightCheck.from_dict(checks_item_data)

            checks.append(checks_item)

        host_id = UUID(d.pop("host_id"))

        host_name = d.pop("host_name")

        resolved_image_ref = d.pop("resolved_image_ref")

        vm_image_preflight_response = cls(
            architecture=architecture,
            backend=backend,
            bootable=bootable,
            checks=checks,
            host_id=host_id,
            host_name=host_name,
            resolved_image_ref=resolved_image_ref,
        )

        vm_image_preflight_response.additional_properties = d
        return vm_image_preflight_response

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
