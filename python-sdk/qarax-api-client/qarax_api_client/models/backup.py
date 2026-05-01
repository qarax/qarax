from __future__ import annotations

import datetime
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from dateutil.parser import isoparse

from ..models.backup_status import BackupStatus
from ..models.backup_type import BackupType
from ..types import UNSET, Unset

T = TypeVar("T", bound="Backup")


@_attrs_define
class Backup:
    """
    Attributes:
        backup_type (BackupType):
        created_at (datetime.datetime):
        id (UUID):
        name (str):
        status (BackupStatus):
        storage_object_id (UUID):
        updated_at (datetime.datetime):
        error_message (None | str | Unset):
        snapshot_id (None | Unset | UUID):
        vm_id (None | Unset | UUID):
    """

    backup_type: BackupType
    created_at: datetime.datetime
    id: UUID
    name: str
    status: BackupStatus
    storage_object_id: UUID
    updated_at: datetime.datetime
    error_message: None | str | Unset = UNSET
    snapshot_id: None | Unset | UUID = UNSET
    vm_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        backup_type = self.backup_type.value

        created_at = self.created_at.isoformat()

        id = str(self.id)

        name = self.name

        status = self.status.value

        storage_object_id = str(self.storage_object_id)

        updated_at = self.updated_at.isoformat()

        error_message: None | str | Unset
        if isinstance(self.error_message, Unset):
            error_message = UNSET
        else:
            error_message = self.error_message

        snapshot_id: None | str | Unset
        if isinstance(self.snapshot_id, Unset):
            snapshot_id = UNSET
        elif isinstance(self.snapshot_id, UUID):
            snapshot_id = str(self.snapshot_id)
        else:
            snapshot_id = self.snapshot_id

        vm_id: None | str | Unset
        if isinstance(self.vm_id, Unset):
            vm_id = UNSET
        elif isinstance(self.vm_id, UUID):
            vm_id = str(self.vm_id)
        else:
            vm_id = self.vm_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "backup_type": backup_type,
                "created_at": created_at,
                "id": id,
                "name": name,
                "status": status,
                "storage_object_id": storage_object_id,
                "updated_at": updated_at,
            }
        )
        if error_message is not UNSET:
            field_dict["error_message"] = error_message
        if snapshot_id is not UNSET:
            field_dict["snapshot_id"] = snapshot_id
        if vm_id is not UNSET:
            field_dict["vm_id"] = vm_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        backup_type = BackupType(d.pop("backup_type"))

        created_at = isoparse(d.pop("created_at"))

        id = UUID(d.pop("id"))

        name = d.pop("name")

        status = BackupStatus(d.pop("status"))

        storage_object_id = UUID(d.pop("storage_object_id"))

        updated_at = isoparse(d.pop("updated_at"))

        def _parse_error_message(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error_message = _parse_error_message(d.pop("error_message", UNSET))

        def _parse_snapshot_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                snapshot_id_type_0 = UUID(data)

                return snapshot_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        snapshot_id = _parse_snapshot_id(d.pop("snapshot_id", UNSET))

        def _parse_vm_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                vm_id_type_0 = UUID(data)

                return vm_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        vm_id = _parse_vm_id(d.pop("vm_id", UNSET))

        backup = cls(
            backup_type=backup_type,
            created_at=created_at,
            id=id,
            name=name,
            status=status,
            storage_object_id=storage_object_id,
            updated_at=updated_at,
            error_message=error_message,
            snapshot_id=snapshot_id,
            vm_id=vm_id,
        )

        backup.additional_properties = d
        return backup

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
