from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="CommitVmRequest")


@_attrs_define
class CommitVmRequest:
    """
    Attributes:
        size_bytes (int): Size of the committed disk in bytes.
        storage_pool_id (UUID): Storage pool to create the raw disk on (must be Local or NFS, attached to the VM's
            host).
    """

    size_bytes: int
    storage_pool_id: UUID
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        size_bytes = self.size_bytes

        storage_pool_id = str(self.storage_pool_id)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "size_bytes": size_bytes,
                "storage_pool_id": storage_pool_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        size_bytes = d.pop("size_bytes")

        storage_pool_id = UUID(d.pop("storage_pool_id"))

        commit_vm_request = cls(
            size_bytes=size_bytes,
            storage_pool_id=storage_pool_id,
        )

        commit_vm_request.additional_properties = d
        return commit_vm_request

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
