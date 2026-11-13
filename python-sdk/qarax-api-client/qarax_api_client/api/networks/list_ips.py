from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.ip_allocation import IpAllocation
from ...types import Response


def _get_kwargs(
    network_id: UUID,
) -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/networks/{network_id}/ips".format(
            network_id=quote(str(network_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | list[IpAllocation] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = IpAllocation.from_dict(response_200_item_data)

            response_200.append(response_200_item)

        return response_200

    if response.status_code == 500:
        response_500 = cast(Any, None)
        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | list[IpAllocation]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    network_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | list[IpAllocation]]:
    """
    Args:
        network_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[IpAllocation]]
    """

    kwargs = _get_kwargs(
        network_id=network_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    network_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | list[IpAllocation] | None:
    """
    Args:
        network_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[IpAllocation]
    """

    return sync_detailed(
        network_id=network_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    network_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | list[IpAllocation]]:
    """
    Args:
        network_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[IpAllocation]]
    """

    kwargs = _get_kwargs(
        network_id=network_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    network_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | list[IpAllocation] | None:
    """
    Args:
        network_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[IpAllocation]
    """

    return (
        await asyncio_detailed(
            network_id=network_id,
            client=client,
        )
    ).parsed
