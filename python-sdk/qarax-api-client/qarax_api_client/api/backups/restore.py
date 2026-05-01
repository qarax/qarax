from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.restore_backup_response import RestoreBackupResponse
from ...types import Response


def _get_kwargs(
    backup_id: UUID,
) -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/backups/{backup_id}/restore".format(
            backup_id=quote(str(backup_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | RestoreBackupResponse | None:
    if response.status_code == 200:
        response_200 = RestoreBackupResponse.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

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


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | RestoreBackupResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    backup_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | RestoreBackupResponse]:
    """
    Args:
        backup_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | RestoreBackupResponse]
    """

    kwargs = _get_kwargs(
        backup_id=backup_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    backup_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | RestoreBackupResponse | None:
    """
    Args:
        backup_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | RestoreBackupResponse
    """

    return sync_detailed(
        backup_id=backup_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    backup_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | RestoreBackupResponse]:
    """
    Args:
        backup_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | RestoreBackupResponse]
    """

    kwargs = _get_kwargs(
        backup_id=backup_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    backup_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | RestoreBackupResponse | None:
    """
    Args:
        backup_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | RestoreBackupResponse
    """

    return (
        await asyncio_detailed(
            backup_id=backup_id,
            client=client,
        )
    ).parsed
