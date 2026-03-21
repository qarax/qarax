from __future__ import annotations

import datetime
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from dateutil.parser import isoparse

from ..models.hook_scope import HookScope
from ..types import UNSET, Unset

T = TypeVar("T", bound="LifecycleHook")


@_attrs_define
class LifecycleHook:
    """
    Attributes:
        active (bool):
        created_at (datetime.datetime):
        events (list[str]):
        id (UUID):
        name (str):
        scope (HookScope):
        updated_at (datetime.datetime):
        url (str):
        scope_value (None | str | Unset):
        secret (None | str | Unset):
    """

    active: bool
    created_at: datetime.datetime
    events: list[str]
    id: UUID
    name: str
    scope: HookScope
    updated_at: datetime.datetime
    url: str
    scope_value: None | str | Unset = UNSET
    secret: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        active = self.active

        created_at = self.created_at.isoformat()

        events = self.events

        id = str(self.id)

        name = self.name

        scope = self.scope.value

        updated_at = self.updated_at.isoformat()

        url = self.url

        scope_value: None | str | Unset
        if isinstance(self.scope_value, Unset):
            scope_value = UNSET
        else:
            scope_value = self.scope_value

        secret: None | str | Unset
        if isinstance(self.secret, Unset):
            secret = UNSET
        else:
            secret = self.secret

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "active": active,
                "created_at": created_at,
                "events": events,
                "id": id,
                "name": name,
                "scope": scope,
                "updated_at": updated_at,
                "url": url,
            }
        )
        if scope_value is not UNSET:
            field_dict["scope_value"] = scope_value
        if secret is not UNSET:
            field_dict["secret"] = secret

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        active = d.pop("active")

        created_at = isoparse(d.pop("created_at"))

        events = cast(list[str], d.pop("events"))

        id = UUID(d.pop("id"))

        name = d.pop("name")

        scope = HookScope(d.pop("scope"))

        updated_at = isoparse(d.pop("updated_at"))

        url = d.pop("url")

        def _parse_scope_value(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        scope_value = _parse_scope_value(d.pop("scope_value", UNSET))

        def _parse_secret(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        secret = _parse_secret(d.pop("secret", UNSET))

        lifecycle_hook = cls(
            active=active,
            created_at=created_at,
            events=events,
            id=id,
            name=name,
            scope=scope,
            updated_at=updated_at,
            url=url,
            scope_value=scope_value,
            secret=secret,
        )

        lifecycle_hook.additional_properties = d
        return lifecycle_hook

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
