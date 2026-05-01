from __future__ import annotations

from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.backup_type import BackupType
from ..types import UNSET, Unset

T = TypeVar("T", bound="RestoreBackupResponse")


@_attrs_define
class RestoreBackupResponse:
    """
    Attributes:
        backup_id (UUID):
        backup_type (BackupType):
        database_name (None | str | Unset):
        vm_id (None | Unset | UUID):
    """

    backup_id: UUID
    backup_type: BackupType
    database_name: None | str | Unset = UNSET
    vm_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        backup_id = str(self.backup_id)

        backup_type = self.backup_type.value

        database_name: None | str | Unset
        if isinstance(self.database_name, Unset):
            database_name = UNSET
        else:
            database_name = self.database_name

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
                "backup_id": backup_id,
                "backup_type": backup_type,
            }
        )
        if database_name is not UNSET:
            field_dict["database_name"] = database_name
        if vm_id is not UNSET:
            field_dict["vm_id"] = vm_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        backup_id = UUID(d.pop("backup_id"))

        backup_type = BackupType(d.pop("backup_type"))

        def _parse_database_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        database_name = _parse_database_name(d.pop("database_name", UNSET))

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

        restore_backup_response = cls(
            backup_id=backup_id,
            backup_type=backup_type,
            database_name=database_name,
            vm_id=vm_id,
        )

        restore_backup_response.additional_properties = d
        return restore_backup_response

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
