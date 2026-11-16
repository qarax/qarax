from http import HTTPStatus
from typing import Any, cast
from uuid import UUID

import httpx2

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.audit_action import AuditAction
from ...models.audit_log_page import AuditLogPage
from ...models.audit_resource_type import AuditResourceType
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    resource_type: AuditResourceType | None | Unset = UNSET,
    resource_id: None | Unset | UUID = UNSET,
    action: AuditAction | None | Unset = UNSET,
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
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

    params["limit"] = limit

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/audit-logs",
        "params": params,
    }

    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx2.Response) -> Any | AuditLogPage | None:
    if response.status_code == 200:
        response_200 = AuditLogPage.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = cast(Any, None)
        return response_400

    if response.status_code == 422:
        response_422 = cast(Any, None)
        return response_422

    if response.status_code == 500:
        response_500 = cast(Any, None)
        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx2.Response) -> Response[Any | AuditLogPage]:
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
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
) -> Response[Any | AuditLogPage]:
    """
    Args:
        resource_type (AuditResourceType | None | Unset):
        resource_id (None | Unset | UUID):
        action (AuditAction | None | Unset):
        limit (int | Unset):
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx2.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | AuditLogPage]
    """

    kwargs = _get_kwargs(
        resource_type=resource_type,
        resource_id=resource_id,
        action=action,
        limit=limit,
        cursor=cursor,
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
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
) -> Any | AuditLogPage | None:
    """
    Args:
        resource_type (AuditResourceType | None | Unset):
        resource_id (None | Unset | UUID):
        action (AuditAction | None | Unset):
        limit (int | Unset):
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx2.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | AuditLogPage
    """

    return sync_detailed(
        client=client,
        resource_type=resource_type,
        resource_id=resource_id,
        action=action,
        limit=limit,
        cursor=cursor,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    resource_type: AuditResourceType | None | Unset = UNSET,
    resource_id: None | Unset | UUID = UNSET,
    action: AuditAction | None | Unset = UNSET,
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
) -> Response[Any | AuditLogPage]:
    """
    Args:
        resource_type (AuditResourceType | None | Unset):
        resource_id (None | Unset | UUID):
        action (AuditAction | None | Unset):
        limit (int | Unset):
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx2.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | AuditLogPage]
    """

    kwargs = _get_kwargs(
        resource_type=resource_type,
        resource_id=resource_id,
        action=action,
        limit=limit,
        cursor=cursor,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    resource_type: AuditResourceType | None | Unset = UNSET,
    resource_id: None | Unset | UUID = UNSET,
    action: AuditAction | None | Unset = UNSET,
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
) -> Any | AuditLogPage | None:
    """
    Args:
        resource_type (AuditResourceType | None | Unset):
        resource_id (None | Unset | UUID):
        action (AuditAction | None | Unset):
        limit (int | Unset):
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx2.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | AuditLogPage
    """

    return (
        await asyncio_detailed(
            client=client,
            resource_type=resource_type,
            resource_id=resource_id,
            action=action,
            limit=limit,
            cursor=cursor,
        )
    ).parsed
