from http import HTTPStatus
from typing import Any, cast
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.audit_action import AuditAction
from ...models.audit_log import AuditLog
from ...models.audit_resource_type import AuditResourceType
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    resource_type: AuditResourceType | None | Unset = UNSET,
    resource_id: None | Unset | UUID = UNSET,
    action: AuditAction | None | Unset = UNSET,
    limit: int | None | Unset = UNSET,
) -> dict[str, Any]:
    params: dict[str, Any] = {}

    json_resource_type: None | str | Unset
    if isinstance(resource_type, Unset):
        json_resource_type = UNSET
    elif isinstance(resource_type, AuditResourceType):
        json_resource_type = resource_type.value
    else:
        json_resource_type = resource_type
    params["resource_type"] = json_resource_type

    json_resource_id: None | str | Unset
    if isinstance(resource_id, Unset):
        json_resource_id = UNSET
    elif isinstance(resource_id, UUID):
        json_resource_id = str(resource_id)
    else:
        json_resource_id = resource_id
    params["resource_id"] = json_resource_id

    json_action: None | str | Unset
    if isinstance(action, Unset):
        json_action = UNSET
    elif isinstance(action, AuditAction):
        json_action = action.value
    else:
        json_action = action
    params["action"] = json_action

    json_limit: int | None | Unset
    if isinstance(limit, Unset):
        json_limit = UNSET
    else:
        json_limit = limit
    params["limit"] = json_limit

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/audit-logs",
        "params": params,
    }

    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | list[AuditLog] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = AuditLog.from_dict(response_200_item_data)

            response_200.append(response_200_item)

        return response_200

    if response.status_code == 400:
        response_400 = cast(Any, None)
        return response_400

    if response.status_code == 500:
        response_500 = cast(Any, None)
        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | list[AuditLog]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    resource_type: AuditResourceType | None | Unset = UNSET,
    resource_id: None | Unset | UUID = UNSET,
    action: AuditAction | None | Unset = UNSET,
    limit: int | None | Unset = UNSET,
) -> Response[Any | list[AuditLog]]:
    """
    Args:
        resource_type (AuditResourceType | None | Unset):
        resource_id (None | Unset | UUID):
        action (AuditAction | None | Unset):
        limit (int | None | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[AuditLog]]
    """

    kwargs = _get_kwargs(
        resource_type=resource_type,
        resource_id=resource_id,
        action=action,
        limit=limit,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    resource_type: AuditResourceType | None | Unset = UNSET,
    resource_id: None | Unset | UUID = UNSET,
    action: AuditAction | None | Unset = UNSET,
    limit: int | None | Unset = UNSET,
) -> Any | list[AuditLog] | None:
    """
    Args:
        resource_type (AuditResourceType | None | Unset):
        resource_id (None | Unset | UUID):
        action (AuditAction | None | Unset):
        limit (int | None | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[AuditLog]
    """

    return sync_detailed(
        client=client,
        resource_type=resource_type,
        resource_id=resource_id,
        action=action,
        limit=limit,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    resource_type: AuditResourceType | None | Unset = UNSET,
    resource_id: None | Unset | UUID = UNSET,
    action: AuditAction | None | Unset = UNSET,
    limit: int | None | Unset = UNSET,
) -> Response[Any | list[AuditLog]]:
    """
    Args:
        resource_type (AuditResourceType | None | Unset):
        resource_id (None | Unset | UUID):
        action (AuditAction | None | Unset):
        limit (int | None | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[AuditLog]]
    """

    kwargs = _get_kwargs(
        resource_type=resource_type,
        resource_id=resource_id,
        action=action,
        limit=limit,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    resource_type: AuditResourceType | None | Unset = UNSET,
    resource_id: None | Unset | UUID = UNSET,
    action: AuditAction | None | Unset = UNSET,
    limit: int | None | Unset = UNSET,
) -> Any | list[AuditLog] | None:
    """
    Args:
        resource_type (AuditResourceType | None | Unset):
        resource_id (None | Unset | UUID):
        action (AuditAction | None | Unset):
        limit (int | None | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[AuditLog]
    """

    return (
        await asyncio_detailed(
            client=client,
            resource_type=resource_type,
            resource_id=resource_id,
            action=action,
            limit=limit,
        )
    ).parsed
