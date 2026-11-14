from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.storage_object_type import StorageObjectType
from ..models.transfer_status import TransferStatus
from ..models.transfer_type import TransferType
from ..types import UNSET, Unset

T = TypeVar("T", bound="Transfer")


@_attrs_define
class Transfer:
    """
    Attributes:
        id (UUID):
        name (str):
        object_type (StorageObjectType):
        source (str):
        status (TransferStatus):
        storage_pool_id (UUID):
        transfer_type (TransferType):
        transferred_bytes (int):
        completed_at (datetime.datetime | None | Unset):
        created_at (datetime.datetime | None | Unset):
        error_message (None | str | Unset):
        started_at (datetime.datetime | None | Unset):
        storage_object_id (None | Unset | UUID):
        total_bytes (int | None | Unset):
        updated_at (datetime.datetime | None | Unset):
    """

    id: UUID
    name: str
    object_type: StorageObjectType
    source: str
    status: TransferStatus
    storage_pool_id: UUID
    transfer_type: TransferType
    transferred_bytes: int
    completed_at: datetime.datetime | None | Unset = UNSET
    created_at: datetime.datetime | None | Unset = UNSET
    error_message: None | str | Unset = UNSET
    started_at: datetime.datetime | None | Unset = UNSET
    storage_object_id: None | Unset | UUID = UNSET
    total_bytes: int | None | Unset = UNSET
    updated_at: datetime.datetime | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = str(self.id)

        name = self.name

        object_type = self.object_type.value

        source = self.source

        status = self.status.value

        storage_pool_id = str(self.storage_pool_id)

        transfer_type = self.transfer_type.value

        transferred_bytes = self.transferred_bytes

        completed_at: None | str | Unset
        if isinstance(self.completed_at, Unset):
            completed_at = UNSET
        elif isinstance(self.completed_at, datetime.datetime):
            completed_at = self.completed_at.isoformat()
        else:
            completed_at = self.completed_at

        created_at: None | str | Unset
        if isinstance(self.created_at, Unset):
            created_at = UNSET
        elif isinstance(self.created_at, datetime.datetime):
            created_at = self.created_at.isoformat()
        else:
            created_at = self.created_at

        error_message: None | str | Unset
        if isinstance(self.error_message, Unset):
            error_message = UNSET
        else:
            error_message = self.error_message

        started_at: None | str | Unset
        if isinstance(self.started_at, Unset):
            started_at = UNSET
        elif isinstance(self.started_at, datetime.datetime):
            started_at = self.started_at.isoformat()
        else:
            started_at = self.started_at

        storage_object_id: None | str | Unset
        if isinstance(self.storage_object_id, Unset):
            storage_object_id = UNSET
        elif isinstance(self.storage_object_id, UUID):
            storage_object_id = str(self.storage_object_id)
        else:
            storage_object_id = self.storage_object_id

        total_bytes: int | None | Unset
        if isinstance(self.total_bytes, Unset):
            total_bytes = UNSET
        else:
            total_bytes = self.total_bytes

        updated_at: None | str | Unset
        if isinstance(self.updated_at, Unset):
            updated_at = UNSET
        elif isinstance(self.updated_at, datetime.datetime):
            updated_at = self.updated_at.isoformat()
        else:
            updated_at = self.updated_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "object_type": object_type,
                "source": source,
                "status": status,
                "storage_pool_id": storage_pool_id,
                "transfer_type": transfer_type,
                "transferred_bytes": transferred_bytes,
            }
        )
        if completed_at is not UNSET:
            field_dict["completed_at"] = completed_at
        if created_at is not UNSET:
            field_dict["created_at"] = created_at
        if error_message is not UNSET:
            field_dict["error_message"] = error_message
        if started_at is not UNSET:
            field_dict["started_at"] = started_at
        if storage_object_id is not UNSET:
            field_dict["storage_object_id"] = storage_object_id
        if total_bytes is not UNSET:
            field_dict["total_bytes"] = total_bytes
        if updated_at is not UNSET:
            field_dict["updated_at"] = updated_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = UUID(d.pop("id"))

        name = d.pop("name")

        object_type = StorageObjectType(d.pop("object_type"))

        source = d.pop("source")

        status = TransferStatus(d.pop("status"))

        storage_pool_id = UUID(d.pop("storage_pool_id"))

        transfer_type = TransferType(d.pop("transfer_type"))

        transferred_bytes = d.pop("transferred_bytes")

        def _parse_completed_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                completed_at_type_0 = datetime.datetime.fromisoformat(data)

                return completed_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        completed_at = _parse_completed_at(d.pop("completed_at", UNSET))

        def _parse_created_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                created_at_type_0 = datetime.datetime.fromisoformat(data)

                return created_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        created_at = _parse_created_at(d.pop("created_at", UNSET))

        def _parse_error_message(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error_message = _parse_error_message(d.pop("error_message", UNSET))

        def _parse_started_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                started_at_type_0 = datetime.datetime.fromisoformat(data)

                return started_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        started_at = _parse_started_at(d.pop("started_at", UNSET))

        def _parse_storage_object_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                storage_object_id_type_0 = UUID(data)

                return storage_object_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        storage_object_id = _parse_storage_object_id(d.pop("storage_object_id", UNSET))

        def _parse_total_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        total_bytes = _parse_total_bytes(d.pop("total_bytes", UNSET))

        def _parse_updated_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                updated_at_type_0 = datetime.datetime.fromisoformat(data)

                return updated_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        updated_at = _parse_updated_at(d.pop("updated_at", UNSET))

        transfer = cls(
            id=id,
            name=name,
            object_type=object_type,
            source=source,
            status=status,
            storage_pool_id=storage_pool_id,
            transfer_type=transfer_type,
            transferred_bytes=transferred_bytes,
            completed_at=completed_at,
            created_at=created_at,
            error_message=error_message,
            started_at=started_at,
            storage_object_id=storage_object_id,
            total_bytes=total_bytes,
            updated_at=updated_at,
        )

        transfer.additional_properties = d
        return transfer

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
