from __future__ import annotations

from typing import Any, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="VmMigrateRequest")


@_attrs_define
class VmMigrateRequest:
    """Request body for `POST /vms/{vm_id}/migrate`.

    Attributes:
        target_host_id (UUID): UUID of the destination host to migrate the VM to.
    """

    target_host_id: UUID
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        target_host_id = str(self.target_host_id)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "target_host_id": target_host_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        target_host_id = UUID(d.pop("target_host_id"))

        vm_migrate_request = cls(
            target_host_id=target_host_id,
        )

        vm_migrate_request.additional_properties = d
        return vm_migrate_request

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
