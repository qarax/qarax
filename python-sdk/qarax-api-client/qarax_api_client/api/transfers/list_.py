from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.transfer import Transfer
from ...types import UNSET, Response, Unset


def _get_kwargs(
    pool_id: UUID,
    *,
    name: None | str | Unset = UNSET,
) -> dict[str, Any]:
    params: dict[str, Any] = {}

    json_name: None | str | Unset
    if isinstance(name, Unset):
        json_name = UNSET
    else:
        json_name = name
    params["name"] = json_name

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/storage-pools/{pool_id}/transfers".format(
            pool_id=quote(str(pool_id), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | list[Transfer] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = Transfer.from_dict(response_200_item_data)

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
) -> Response[Any | list[Transfer]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    pool_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
) -> Response[Any | list[Transfer]]:
    """
    Args:
        pool_id (UUID):
        name (None | str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[Transfer]]
    """

    kwargs = _get_kwargs(
        pool_id=pool_id,
        name=name,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    pool_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
) -> Any | list[Transfer] | None:
    """
    Args:
        pool_id (UUID):
        name (None | str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[Transfer]
    """

    return sync_detailed(
        pool_id=pool_id,
        client=client,
        name=name,
    ).parsed


async def asyncio_detailed(
    pool_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
) -> Response[Any | list[Transfer]]:
    """
    Args:
        pool_id (UUID):
        name (None | str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[Transfer]]
    """

    kwargs = _get_kwargs(
        pool_id=pool_id,
        name=name,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    pool_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
) -> Any | list[Transfer] | None:
    """
    Args:
        pool_id (UUID):
        name (None | str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[Transfer]
    """

    return (
        await asyncio_detailed(
            pool_id=pool_id,
            client=client,
            name=name,
        )
    ).parsed
