from __future__ import annotations

from typing import Any, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RestoreRequest")


@_attrs_define
class RestoreRequest:
    """
    Attributes:
        snapshot_id (UUID):
    """

    snapshot_id: UUID
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        snapshot_id = str(self.snapshot_id)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "snapshot_id": snapshot_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        snapshot_id = UUID(d.pop("snapshot_id"))

        restore_request = cls(
            snapshot_id=snapshot_id,
        )

        restore_request.additional_properties = d
        return restore_request

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
