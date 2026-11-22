from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ConfigureSandboxPoolRequest")


@_attrs_define
class ConfigureSandboxPoolRequest:
    """
    Attributes:
        min_ready (int): How many sandboxes the pool keeps ready to hand out.
        fork (bool | Unset): Make the pool's members by forking one VM of the template it keeps
            booted, its seed, rather than booting each one: a member is ready in
            milliseconds and shares the seed's memory until it writes it. A
            sandbox asked for when the pool has no member ready is forked from the
            seed on the request, so with `min_ready` 0 the pool keeps only its seed.
    """

    min_ready: int
    fork: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        min_ready = self.min_ready

        fork = self.fork

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "min_ready": min_ready,
            }
        )
        if fork is not UNSET:
            field_dict["fork"] = fork

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        min_ready = d.pop("min_ready")

        fork = d.pop("fork", UNSET)

        configure_sandbox_pool_request = cls(
            min_ready=min_ready,
            fork=fork,
        )

        configure_sandbox_pool_request.additional_properties = d
        return configure_sandbox_pool_request

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
