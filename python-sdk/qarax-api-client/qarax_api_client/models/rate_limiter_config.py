from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.token_bucket import TokenBucket


T = TypeVar("T", bound="RateLimiterConfig")


@_attrs_define
class RateLimiterConfig:
    """
    Attributes:
        bandwidth (None | TokenBucket | Unset):
        ops (None | TokenBucket | Unset):
    """

    bandwidth: None | TokenBucket | Unset = UNSET
    ops: None | TokenBucket | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.token_bucket import TokenBucket

        bandwidth: dict[str, Any] | None | Unset
        if isinstance(self.bandwidth, Unset):
            bandwidth = UNSET
        elif isinstance(self.bandwidth, TokenBucket):
            bandwidth = self.bandwidth.to_dict()
        else:
            bandwidth = self.bandwidth

        ops: dict[str, Any] | None | Unset
        if isinstance(self.ops, Unset):
            ops = UNSET
        elif isinstance(self.ops, TokenBucket):
            ops = self.ops.to_dict()
        else:
            ops = self.ops

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if bandwidth is not UNSET:
            field_dict["bandwidth"] = bandwidth
        if ops is not UNSET:
            field_dict["ops"] = ops

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        from ..models.token_bucket import TokenBucket

        d = dict(src_dict)

        def _parse_bandwidth(data: object) -> None | TokenBucket | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                bandwidth_type_0 = TokenBucket.from_dict(data)

                return bandwidth_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | TokenBucket | Unset, data)

        bandwidth = _parse_bandwidth(d.pop("bandwidth", UNSET))

        def _parse_ops(data: object) -> None | TokenBucket | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                ops_type_0 = TokenBucket.from_dict(data)

                return ops_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | TokenBucket | Unset, data)

        ops = _parse_ops(d.pop("ops", UNSET))

        rate_limiter_config = cls(
            bandwidth=bandwidth,
            ops=ops,
        )

        rate_limiter_config.additional_properties = d
        return rate_limiter_config

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
