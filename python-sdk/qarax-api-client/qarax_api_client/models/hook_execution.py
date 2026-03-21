from __future__ import annotations

import datetime
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field
from dateutil.parser import isoparse

from ..models.hook_execution_status import HookExecutionStatus
from ..types import UNSET, Unset

T = TypeVar("T", bound="HookExecution")


@_attrs_define
class HookExecution:
    """
    Attributes:
        attempt_count (int):
        created_at (datetime.datetime):
        hook_id (UUID):
        id (UUID):
        max_attempts (int):
        new_status (str):
        next_retry_at (datetime.datetime):
        payload (Any):
        previous_status (str):
        status (HookExecutionStatus):
        vm_id (UUID):
        delivered_at (datetime.datetime | None | Unset):
        last_error (None | str | Unset):
        response_body (None | str | Unset):
        response_status (int | None | Unset):
    """

    attempt_count: int
    created_at: datetime.datetime
    hook_id: UUID
    id: UUID
    max_attempts: int
    new_status: str
    next_retry_at: datetime.datetime
    payload: Any
    previous_status: str
    status: HookExecutionStatus
    vm_id: UUID
    delivered_at: datetime.datetime | None | Unset = UNSET
    last_error: None | str | Unset = UNSET
    response_body: None | str | Unset = UNSET
    response_status: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        attempt_count = self.attempt_count

        created_at = self.created_at.isoformat()

        hook_id = str(self.hook_id)

        id = str(self.id)

        max_attempts = self.max_attempts

        new_status = self.new_status

        next_retry_at = self.next_retry_at.isoformat()

        payload = self.payload

        previous_status = self.previous_status

        status = self.status.value

        vm_id = str(self.vm_id)

        delivered_at: None | str | Unset
        if isinstance(self.delivered_at, Unset):
            delivered_at = UNSET
        elif isinstance(self.delivered_at, datetime.datetime):
            delivered_at = self.delivered_at.isoformat()
        else:
            delivered_at = self.delivered_at

        last_error: None | str | Unset
        if isinstance(self.last_error, Unset):
            last_error = UNSET
        else:
            last_error = self.last_error

        response_body: None | str | Unset
        if isinstance(self.response_body, Unset):
            response_body = UNSET
        else:
            response_body = self.response_body

        response_status: int | None | Unset
        if isinstance(self.response_status, Unset):
            response_status = UNSET
        else:
            response_status = self.response_status

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "attempt_count": attempt_count,
                "created_at": created_at,
                "hook_id": hook_id,
                "id": id,
                "max_attempts": max_attempts,
                "new_status": new_status,
                "next_retry_at": next_retry_at,
                "payload": payload,
                "previous_status": previous_status,
                "status": status,
                "vm_id": vm_id,
            }
        )
        if delivered_at is not UNSET:
            field_dict["delivered_at"] = delivered_at
        if last_error is not UNSET:
            field_dict["last_error"] = last_error
        if response_body is not UNSET:
            field_dict["response_body"] = response_body
        if response_status is not UNSET:
            field_dict["response_status"] = response_status

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        attempt_count = d.pop("attempt_count")

        created_at = isoparse(d.pop("created_at"))

        hook_id = UUID(d.pop("hook_id"))

        id = UUID(d.pop("id"))

        max_attempts = d.pop("max_attempts")

        new_status = d.pop("new_status")

        next_retry_at = isoparse(d.pop("next_retry_at"))

        payload = d.pop("payload")

        previous_status = d.pop("previous_status")

        status = HookExecutionStatus(d.pop("status"))

        vm_id = UUID(d.pop("vm_id"))

        def _parse_delivered_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                delivered_at_type_0 = isoparse(data)

                return delivered_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        delivered_at = _parse_delivered_at(d.pop("delivered_at", UNSET))

        def _parse_last_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        last_error = _parse_last_error(d.pop("last_error", UNSET))

        def _parse_response_body(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        response_body = _parse_response_body(d.pop("response_body", UNSET))

        def _parse_response_status(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        response_status = _parse_response_status(d.pop("response_status", UNSET))

        hook_execution = cls(
            attempt_count=attempt_count,
            created_at=created_at,
            hook_id=hook_id,
            id=id,
            max_attempts=max_attempts,
            new_status=new_status,
            next_retry_at=next_retry_at,
            payload=payload,
            previous_status=previous_status,
            status=status,
            vm_id=vm_id,
            delivered_at=delivered_at,
            last_error=last_error,
            response_body=response_body,
            response_status=response_status,
        )

        hook_execution.additional_properties = d
        return hook_execution

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
