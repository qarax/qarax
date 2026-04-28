from __future__ import annotations

from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ConfigureSandboxPoolRequest")


@_attrs_define
class ConfigureSandboxPoolRequest:
    """
    Attributes:
        min_ready (int):
    """

    min_ready: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        min_ready = self.min_ready

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "min_ready": min_ready,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        min_ready = d.pop("min_ready")

        configure_sandbox_pool_request = cls(
            min_ready=min_ready,
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
