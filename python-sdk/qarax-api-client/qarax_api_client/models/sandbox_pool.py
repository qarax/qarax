from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.sandbox_pool_member_status import SandboxPoolMemberStatus
from ..types import UNSET, Unset

T = TypeVar("T", bound="SandboxPool")


@_attrs_define
class SandboxPool:
    """
    Attributes:
        created_at (datetime.datetime):
        current_error (int):
        current_provisioning (int):
        current_ready (int): Counts of the pool's members, not counting its seed.
        fork (bool): The pool makes its members by forking its seed, one VM of the template
            it keeps booted, rather than booting each one, and forks a sandbox from
            the seed when it has no member ready.
        id (UUID):
        min_ready (int):
        updated_at (datetime.datetime):
        vm_template_id (UUID):
        vm_template_name (str):
        seed_status (None | SandboxPoolMemberStatus | Unset):
        seed_vm_id (None | Unset | UUID): The seed's VM, while the pool has one.
    """

    created_at: datetime.datetime
    current_error: int
    current_provisioning: int
    current_ready: int
    fork: bool
    id: UUID
    min_ready: int
    updated_at: datetime.datetime
    vm_template_id: UUID
    vm_template_name: str
    seed_status: None | SandboxPoolMemberStatus | Unset = UNSET
    seed_vm_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at.isoformat()

        current_error = self.current_error

        current_provisioning = self.current_provisioning

        current_ready = self.current_ready

        fork = self.fork

        id = str(self.id)

        min_ready = self.min_ready

        updated_at = self.updated_at.isoformat()

        vm_template_id = str(self.vm_template_id)

        vm_template_name = self.vm_template_name

        seed_status: None | str | Unset
        if isinstance(self.seed_status, Unset):
            seed_status = UNSET
        elif isinstance(self.seed_status, SandboxPoolMemberStatus):
            seed_status = self.seed_status.value
        else:
            seed_status = self.seed_status

        seed_vm_id: None | str | Unset
        if isinstance(self.seed_vm_id, Unset):
            seed_vm_id = UNSET
        elif isinstance(self.seed_vm_id, UUID):
            seed_vm_id = str(self.seed_vm_id)
        else:
            seed_vm_id = self.seed_vm_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "created_at": created_at,
                "current_error": current_error,
                "current_provisioning": current_provisioning,
                "current_ready": current_ready,
                "fork": fork,
                "id": id,
                "min_ready": min_ready,
                "updated_at": updated_at,
                "vm_template_id": vm_template_id,
                "vm_template_name": vm_template_name,
            }
        )
        if seed_status is not UNSET:
            field_dict["seed_status"] = seed_status
        if seed_vm_id is not UNSET:
            field_dict["seed_vm_id"] = seed_vm_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        created_at = datetime.datetime.fromisoformat(d.pop("created_at"))

        current_error = d.pop("current_error")

        current_provisioning = d.pop("current_provisioning")

        current_ready = d.pop("current_ready")

        fork = d.pop("fork")

        id = UUID(d.pop("id"))

        min_ready = d.pop("min_ready")

        updated_at = datetime.datetime.fromisoformat(d.pop("updated_at"))

        vm_template_id = UUID(d.pop("vm_template_id"))

        vm_template_name = d.pop("vm_template_name")

        def _parse_seed_status(data: object) -> None | SandboxPoolMemberStatus | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                seed_status_type_0 = SandboxPoolMemberStatus(data)

                return seed_status_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SandboxPoolMemberStatus | Unset, data)

        seed_status = _parse_seed_status(d.pop("seed_status", UNSET))

        def _parse_seed_vm_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                seed_vm_id_type_0 = UUID(data)

                return seed_vm_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        seed_vm_id = _parse_seed_vm_id(d.pop("seed_vm_id", UNSET))

        sandbox_pool = cls(
            created_at=created_at,
            current_error=current_error,
            current_provisioning=current_provisioning,
            current_ready=current_ready,
            fork=fork,
            id=id,
            min_ready=min_ready,
            updated_at=updated_at,
            vm_template_id=vm_template_id,
            vm_template_name=vm_template_name,
            seed_status=seed_status,
            seed_vm_id=seed_vm_id,
        )

        sandbox_pool.additional_properties = d
        return sandbox_pool

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
