from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="CreateSnapshotRequest")


@_attrs_define
class CreateSnapshotRequest:
    """
    Attributes:
        name (None | str | Unset): Human-readable name for the snapshot (auto-generated if omitted).
        storage_pool_id (None | Unset | UUID): Storage pool to place the snapshot in. Defaults to the pool of the
            VM's primary disk, or any active non-OverlayBD pool.
    """

    name: None | str | Unset = UNSET
    storage_pool_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name: None | str | Unset
        if isinstance(self.name, Unset):
            name = UNSET
        else:
            name = self.name

        storage_pool_id: None | str | Unset
        if isinstance(self.storage_pool_id, Unset):
            storage_pool_id = UNSET
        elif isinstance(self.storage_pool_id, UUID):
            storage_pool_id = str(self.storage_pool_id)
        else:
            storage_pool_id = self.storage_pool_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if name is not UNSET:
            field_dict["name"] = name
        if storage_pool_id is not UNSET:
            field_dict["storage_pool_id"] = storage_pool_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        name = _parse_name(d.pop("name", UNSET))

        def _parse_storage_pool_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                storage_pool_id_type_0 = UUID(data)

                return storage_pool_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        storage_pool_id = _parse_storage_pool_id(d.pop("storage_pool_id", UNSET))

        create_snapshot_request = cls(
            name=name,
            storage_pool_id=storage_pool_id,
        )

        create_snapshot_request.additional_properties = d
        return create_snapshot_request

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
