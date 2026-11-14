from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.snapshot_status import SnapshotStatus

T = TypeVar("T", bound="Snapshot")


@_attrs_define
class Snapshot:
    """
    Attributes:
        created_at (datetime.datetime):
        id (UUID):
        name (str):
        status (SnapshotStatus):
        storage_object_id (UUID):
        vm_id (UUID):
    """

    created_at: datetime.datetime
    id: UUID
    name: str
    status: SnapshotStatus
    storage_object_id: UUID
    vm_id: UUID
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at.isoformat()

        id = str(self.id)

        name = self.name

        status = self.status.value

        storage_object_id = str(self.storage_object_id)

        vm_id = str(self.vm_id)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created_at": created_at,
                "id": id,
                "name": name,
                "status": status,
                "storage_object_id": storage_object_id,
                "vm_id": vm_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        created_at = datetime.datetime.fromisoformat(d.pop("created_at"))

        id = UUID(d.pop("id"))

        name = d.pop("name")

        status = SnapshotStatus(d.pop("status"))

        storage_object_id = UUID(d.pop("storage_object_id"))

        vm_id = UUID(d.pop("vm_id"))

        snapshot = cls(
            created_at=created_at,
            id=id,
            name=name,
            status=status,
            storage_object_id=storage_object_id,
            vm_id=vm_id,
        )

        snapshot.additional_properties = d
        return snapshot

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
