from __future__ import annotations

from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.hook_scope import HookScope
from ..types import UNSET, Unset

T = TypeVar("T", bound="NewLifecycleHook")


@_attrs_define
class NewLifecycleHook:
    """
    Attributes:
        name (str):
        url (str):
        events (list[str] | Unset):
        scope (HookScope | Unset):
        scope_value (None | str | Unset):
        secret (None | str | Unset):
    """

    name: str
    url: str
    events: list[str] | Unset = UNSET
    scope: HookScope | Unset = UNSET
    scope_value: None | str | Unset = UNSET
    secret: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        url = self.url

        events: list[str] | Unset = UNSET
        if not isinstance(self.events, Unset):
            events = self.events

        scope: str | Unset = UNSET
        if not isinstance(self.scope, Unset):
            scope = self.scope.value

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
                "name": name,
                "url": url,
            }
        )
        if events is not UNSET:
            field_dict["events"] = events
        if scope is not UNSET:
            field_dict["scope"] = scope
        if scope_value is not UNSET:
            field_dict["scope_value"] = scope_value
        if secret is not UNSET:
            field_dict["secret"] = secret

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        url = d.pop("url")

        events = cast(list[str], d.pop("events", UNSET))

        _scope = d.pop("scope", UNSET)
        scope: HookScope | Unset
        if isinstance(_scope, Unset):
            scope = UNSET
        else:
            scope = HookScope(_scope)

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

        new_lifecycle_hook = cls(
            name=name,
            url=url,
            events=events,
            scope=scope,
            scope_value=scope_value,
            secret=secret,
        )

        new_lifecycle_hook.additional_properties = d
        return new_lifecycle_hook

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
