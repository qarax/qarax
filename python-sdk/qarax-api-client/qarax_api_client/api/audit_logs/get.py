from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.audit_log import AuditLog
from ...types import Response


def _get_kwargs(
    audit_log_id: UUID,
) -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/audit-logs/{audit_log_id}".format(
            audit_log_id=quote(str(audit_log_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | AuditLog | None:
    if response.status_code == 200:
        response_200 = AuditLog.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if response.status_code == 500:
        response_500 = cast(Any, None)
        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | AuditLog]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    audit_log_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | AuditLog]:
    """
    Args:
        audit_log_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | AuditLog]
    """

    kwargs = _get_kwargs(
        audit_log_id=audit_log_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    audit_log_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | AuditLog | None:
    """
    Args:
        audit_log_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | AuditLog
    """

    return sync_detailed(
        audit_log_id=audit_log_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    audit_log_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | AuditLog]:
    """
    Args:
        audit_log_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | AuditLog]
    """

    kwargs = _get_kwargs(
        audit_log_id=audit_log_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    audit_log_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | AuditLog | None:
    """
    Args:
        audit_log_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | AuditLog
    """

    return (
        await asyncio_detailed(
            audit_log_id=audit_log_id,
            client=client,
        )
    ).parsed
