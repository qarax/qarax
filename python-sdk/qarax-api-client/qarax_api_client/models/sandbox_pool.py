from __future__ import annotations

import datetime
from typing import Any, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from dateutil.parser import isoparse

T = TypeVar("T", bound="SandboxPool")


@_attrs_define
class SandboxPool:
    """
    Attributes:
        created_at (datetime.datetime):
        current_error (int):
        current_provisioning (int):
        current_ready (int):
        id (UUID):
        min_ready (int):
        updated_at (datetime.datetime):
        vm_template_id (UUID):
        vm_template_name (str):
    """

    created_at: datetime.datetime
    current_error: int
    current_provisioning: int
    current_ready: int
    id: UUID
    min_ready: int
    updated_at: datetime.datetime
    vm_template_id: UUID
    vm_template_name: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at.isoformat()

        current_error = self.current_error

        current_provisioning = self.current_provisioning

        current_ready = self.current_ready

        id = str(self.id)

        min_ready = self.min_ready

        updated_at = self.updated_at.isoformat()

        vm_template_id = str(self.vm_template_id)

        vm_template_name = self.vm_template_name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created_at": created_at,
                "current_error": current_error,
                "current_provisioning": current_provisioning,
                "current_ready": current_ready,
                "id": id,
                "min_ready": min_ready,
                "updated_at": updated_at,
                "vm_template_id": vm_template_id,
                "vm_template_name": vm_template_name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        created_at = isoparse(d.pop("created_at"))

        current_error = d.pop("current_error")

        current_provisioning = d.pop("current_provisioning")

        current_ready = d.pop("current_ready")

        id = UUID(d.pop("id"))

        min_ready = d.pop("min_ready")

        updated_at = isoparse(d.pop("updated_at"))

        vm_template_id = UUID(d.pop("vm_template_id"))

        vm_template_name = d.pop("vm_template_name")

        sandbox_pool = cls(
            created_at=created_at,
            current_error=current_error,
            current_provisioning=current_provisioning,
            current_ready=current_ready,
            id=id,
            min_ready=min_ready,
            updated_at=updated_at,
            vm_template_id=vm_template_id,
            vm_template_name=vm_template_name,
        )

        sandbox_pool.additional_properties = d
        return sandbox_pool

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
