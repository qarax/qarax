from __future__ import annotations

from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.hook_scope import HookScope
from ..types import UNSET, Unset

T = TypeVar("T", bound="UpdateLifecycleHook")


@_attrs_define
class UpdateLifecycleHook:
    """
    Attributes:
        active (bool | None | Unset):
        events (list[str] | None | Unset):
        scope (HookScope | None | Unset):
        scope_value (None | str | Unset):
        secret (None | str | Unset):
        url (None | str | Unset):
    """

    active: bool | None | Unset = UNSET
    events: list[str] | None | Unset = UNSET
    scope: HookScope | None | Unset = UNSET
    scope_value: None | str | Unset = UNSET
    secret: None | str | Unset = UNSET
    url: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        active: bool | None | Unset
        if isinstance(self.active, Unset):
            active = UNSET
        else:
            active = self.active

        events: list[str] | None | Unset
        if isinstance(self.events, Unset):
            events = UNSET
        elif isinstance(self.events, list):
            events = self.events

        else:
            events = self.events

        scope: None | str | Unset
        if isinstance(self.scope, Unset):
            scope = UNSET
        elif isinstance(self.scope, HookScope):
            scope = self.scope.value
        else:
            scope = self.scope

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

        url: None | str | Unset
        if isinstance(self.url, Unset):
            url = UNSET
        else:
            url = self.url

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if active is not UNSET:
            field_dict["active"] = active
        if events is not UNSET:
            field_dict["events"] = events
        if scope is not UNSET:
            field_dict["scope"] = scope
        if scope_value is not UNSET:
            field_dict["scope_value"] = scope_value
        if secret is not UNSET:
            field_dict["secret"] = secret
        if url is not UNSET:
            field_dict["url"] = url

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)

        def _parse_active(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        active = _parse_active(d.pop("active", UNSET))

        def _parse_events(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                events_type_0 = cast(list[str], data)

                return events_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        events = _parse_events(d.pop("events", UNSET))

        def _parse_scope(data: object) -> HookScope | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                scope_type_1 = HookScope(data)

                return scope_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(HookScope | None | Unset, data)

        scope = _parse_scope(d.pop("scope", UNSET))

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

        def _parse_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        url = _parse_url(d.pop("url", UNSET))

        update_lifecycle_hook = cls(
            active=active,
            events=events,
            scope=scope,
            scope_value=scope_value,
            secret=secret,
            url=url,
        )

        update_lifecycle_hook.additional_properties = d
        return update_lifecycle_hook

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
