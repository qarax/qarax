from __future__ import annotations

from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="CreateDiskRequest")


@_attrs_define
class CreateDiskRequest:
    """
    Attributes:
        name (str): Human-readable name for the resulting storage object.
        preallocate (bool | Unset): If true, use fallocate to reserve blocks upfront (default: sparse).
        size_bytes (int | None | Unset): Logical size of the disk in bytes. Required when no source_url is given.
            When source_url is provided, this is optional and, if set, is used as the
            initial reported size until the download completes and the actual size is known.
        source_url (None | str | Unset): Optional URL to populate the disk from (e.g. a cloud image). When set the
            operation becomes async and returns 202 with a job_id.
    """

    name: str
    preallocate: bool | Unset = UNSET
    size_bytes: int | None | Unset = UNSET
    source_url: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        preallocate = self.preallocate

        size_bytes: int | None | Unset
        if isinstance(self.size_bytes, Unset):
            size_bytes = UNSET
        else:
            size_bytes = self.size_bytes

        source_url: None | str | Unset
        if isinstance(self.source_url, Unset):
            source_url = UNSET
        else:
            source_url = self.source_url

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
            }
        )
        if preallocate is not UNSET:
            field_dict["preallocate"] = preallocate
        if size_bytes is not UNSET:
            field_dict["size_bytes"] = size_bytes
        if source_url is not UNSET:
            field_dict["source_url"] = source_url

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        preallocate = d.pop("preallocate", UNSET)

        def _parse_size_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        size_bytes = _parse_size_bytes(d.pop("size_bytes", UNSET))

        def _parse_source_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        source_url = _parse_source_url(d.pop("source_url", UNSET))

        create_disk_request = cls(
            name=name,
            preallocate=preallocate,
            size_bytes=size_bytes,
            source_url=source_url,
        )

        create_disk_request.additional_properties = d
        return create_disk_request

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
