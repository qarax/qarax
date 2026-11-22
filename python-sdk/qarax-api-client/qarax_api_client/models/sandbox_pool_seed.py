from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.sandbox_pool_member_status import SandboxPoolMemberStatus
from ..types import UNSET, Unset

T = TypeVar("T", bound="SandboxPoolSeed")


@_attrs_define
class SandboxPoolSeed:
    """A VM of the template a forking pool keeps booted and forks from.

    Attributes:
        status (SandboxPoolMemberStatus):
        vm_id (UUID):
        host_id (None | Unset | UUID): The host it runs on.
    """

    status: SandboxPoolMemberStatus
    vm_id: UUID
    host_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        status = self.status.value

        vm_id = str(self.vm_id)

        host_id: None | str | Unset
        if isinstance(self.host_id, Unset):
            host_id = UNSET
        elif isinstance(self.host_id, UUID):
            host_id = str(self.host_id)
        else:
            host_id = self.host_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "status": status,
                "vm_id": vm_id,
            }
        )
        if host_id is not UNSET:
            field_dict["host_id"] = host_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        status = SandboxPoolMemberStatus(d.pop("status"))

        vm_id = UUID(d.pop("vm_id"))

        def _parse_host_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                host_id_type_0 = UUID(data)

                return host_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        host_id = _parse_host_id(d.pop("host_id", UNSET))

        sandbox_pool_seed = cls(
            status=status,
            vm_id=vm_id,
            host_id=host_id,
        )

        sandbox_pool_seed.additional_properties = d
        return sandbox_pool_seed

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
