from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="TokenBucket")


@_attrs_define
class TokenBucket:
    """
    Attributes:
        refill_time (int):
        size (int):
        one_time_burst (int | None | Unset):
    """

    refill_time: int
    size: int
    one_time_burst: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        refill_time = self.refill_time

        size = self.size

        one_time_burst: int | None | Unset
        if isinstance(self.one_time_burst, Unset):
            one_time_burst = UNSET
        else:
            one_time_burst = self.one_time_burst

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "refill_time": refill_time,
                "size": size,
            }
        )
        if one_time_burst is not UNSET:
            field_dict["one_time_burst"] = one_time_burst

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        refill_time = d.pop("refill_time")

        size = d.pop("size")

        def _parse_one_time_burst(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        one_time_burst = _parse_one_time_burst(d.pop("one_time_burst", UNSET))

        token_bucket = cls(
            refill_time=refill_time,
            size=size,
            one_time_burst=one_time_burst,
        )

        token_bucket.additional_properties = d
        return token_bucket

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
