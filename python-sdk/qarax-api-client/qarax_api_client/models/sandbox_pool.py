from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.sandbox_pool_seed import SandboxPoolSeed


T = TypeVar("T", bound="SandboxPool")


@_attrs_define
class SandboxPool:
    """
    Attributes:
        created_at (datetime.datetime):
        current_error (int):
        current_provisioning (int):
        current_ready (int): Counts of the pool's members, not counting its seeds.
        fork (bool): The pool makes its members by forking its seeds, VMs of the template
            it keeps booted, rather than booting each one, and forks a sandbox from
            a seed when it has no member ready.
        id (UUID):
        min_ready (int):
        seed_hosts (int): How many hosts the pool keeps a seed on, when it forks.
        seeds (list[SandboxPoolSeed]): The pool's seeds, oldest first: at most one on a host.
        updated_at (datetime.datetime):
        vm_template_id (UUID):
        vm_template_name (str):
    """

    created_at: datetime.datetime
    current_error: int
    current_provisioning: int
    current_ready: int
    fork: bool
    id: UUID
    min_ready: int
    seed_hosts: int
    seeds: list[SandboxPoolSeed]
    updated_at: datetime.datetime
    vm_template_id: UUID
    vm_template_name: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        created_at = self.created_at.isoformat()

        current_error = self.current_error

        current_provisioning = self.current_provisioning

        current_ready = self.current_ready

        fork = self.fork

        id = str(self.id)

        min_ready = self.min_ready

        seed_hosts = self.seed_hosts

        seeds = []
        for seeds_item_data in self.seeds:
            seeds_item = seeds_item_data.to_dict()
            seeds.append(seeds_item)

        updated_at = self.updated_at.isoformat()

        vm_template_id = str(self.vm_template_id)

        vm_template_name = self.vm_template_name

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
                "seed_hosts": seed_hosts,
                "seeds": seeds,
                "updated_at": updated_at,
                "vm_template_id": vm_template_id,
                "vm_template_name": vm_template_name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.sandbox_pool_seed import SandboxPoolSeed  # noqa: PLC0415

        d = dict(src_dict)
        created_at = datetime.datetime.fromisoformat(d.pop("created_at"))

        current_error = d.pop("current_error")

        current_provisioning = d.pop("current_provisioning")

        current_ready = d.pop("current_ready")

        fork = d.pop("fork")

        id = UUID(d.pop("id"))

        min_ready = d.pop("min_ready")

        seed_hosts = d.pop("seed_hosts")

        seeds = []
        _seeds = d.pop("seeds")
        for seeds_item_data in _seeds:
            seeds_item = SandboxPoolSeed.from_dict(seeds_item_data)

            seeds.append(seeds_item)

        updated_at = datetime.datetime.fromisoformat(d.pop("updated_at"))

        vm_template_id = UUID(d.pop("vm_template_id"))

        vm_template_name = d.pop("vm_template_name")

        sandbox_pool = cls(
            created_at=created_at,
            current_error=current_error,
            current_provisioning=current_provisioning,
            current_ready=current_ready,
            fork=fork,
            id=id,
            min_ready=min_ready,
            seed_hosts=seed_hosts,
            seeds=seeds,
            updated_at=updated_at,
            vm_template_id=vm_template_id,
            vm_template_name=vm_template_name,
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
