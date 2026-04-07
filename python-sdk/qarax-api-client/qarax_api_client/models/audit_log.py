from __future__ import annotations

import datetime
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from dateutil.parser import isoparse

from ..models.audit_action import AuditAction
from ..models.audit_resource_type import AuditResourceType
from ..types import UNSET, Unset

T = TypeVar("T", bound="AuditLog")


@_attrs_define
class AuditLog:
    """
    Attributes:
        action (AuditAction):
        created_at (datetime.datetime):
        id (UUID):
        resource_id (UUID):
        resource_type (AuditResourceType):
        metadata (Any | Unset):
        resource_name (None | str | Unset):
    """

    action: AuditAction
    created_at: datetime.datetime
    id: UUID
    resource_id: UUID
    resource_type: AuditResourceType
    metadata: Any | Unset = UNSET
    resource_name: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        action = self.action.value

        created_at = self.created_at.isoformat()

        id = str(self.id)

        resource_id = str(self.resource_id)

        resource_type = self.resource_type.value

        metadata = self.metadata

        resource_name: None | str | Unset
        if isinstance(self.resource_name, Unset):
            resource_name = UNSET
        else:
            resource_name = self.resource_name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "action": action,
                "created_at": created_at,
                "id": id,
                "resource_id": resource_id,
                "resource_type": resource_type,
            }
        )
        if metadata is not UNSET:
            field_dict["metadata"] = metadata
        if resource_name is not UNSET:
            field_dict["resource_name"] = resource_name

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        action = AuditAction(d.pop("action"))

        created_at = isoparse(d.pop("created_at"))

        id = UUID(d.pop("id"))

        resource_id = UUID(d.pop("resource_id"))

        resource_type = AuditResourceType(d.pop("resource_type"))

        metadata = d.pop("metadata", UNSET)

        def _parse_resource_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        resource_name = _parse_resource_name(d.pop("resource_name", UNSET))

        audit_log = cls(
            action=action,
            created_at=created_at,
            id=id,
            resource_id=resource_id,
            resource_type=resource_type,
            metadata=metadata,
            resource_name=resource_name,
        )

        audit_log.additional_properties = d
        return audit_log

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
