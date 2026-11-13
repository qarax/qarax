from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.transfer import Transfer
from ...types import Response


def _get_kwargs(
    pool_id: UUID,
    transfer_id: UUID,
) -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/storage-pools/{pool_id}/transfers/{transfer_id}".format(
            pool_id=quote(str(pool_id), safe=""),
            transfer_id=quote(str(transfer_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | Transfer | None:
    if response.status_code == 200:
        response_200 = Transfer.from_dict(response.json())

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


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | Transfer]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    pool_id: UUID,
    transfer_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | Transfer]:
    """
    Args:
        pool_id (UUID):
        transfer_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | Transfer]
    """

    kwargs = _get_kwargs(
        pool_id=pool_id,
        transfer_id=transfer_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    pool_id: UUID,
    transfer_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | Transfer | None:
    """
    Args:
        pool_id (UUID):
        transfer_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | Transfer
    """

    return sync_detailed(
        pool_id=pool_id,
        transfer_id=transfer_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    pool_id: UUID,
    transfer_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | Transfer]:
    """
    Args:
        pool_id (UUID):
        transfer_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | Transfer]
    """

    kwargs = _get_kwargs(
        pool_id=pool_id,
        transfer_id=transfer_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    pool_id: UUID,
    transfer_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | Transfer | None:
    """
    Args:
        pool_id (UUID):
        transfer_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | Transfer
    """

    return (
        await asyncio_detailed(
            pool_id=pool_id,
            transfer_id=transfer_id,
            client=client,
        )
    ).parsed
