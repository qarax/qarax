from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.hook_execution import HookExecution
from ...types import Response


def _get_kwargs(
    hook_id: UUID,
) -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/hooks/{hook_id}/executions".format(
            hook_id=quote(str(hook_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | list[HookExecution] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = HookExecution.from_dict(response_200_item_data)

            response_200.append(response_200_item)

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


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | list[HookExecution]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    hook_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | list[HookExecution]]:
    """
    Args:
        hook_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[HookExecution]]
    """

    kwargs = _get_kwargs(
        hook_id=hook_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    hook_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | list[HookExecution] | None:
    """
    Args:
        hook_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[HookExecution]
    """

    return sync_detailed(
        hook_id=hook_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    hook_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | list[HookExecution]]:
    """
    Args:
        hook_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[HookExecution]]
    """

    kwargs = _get_kwargs(
        hook_id=hook_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    hook_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | list[HookExecution] | None:
    """
    Args:
        hook_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[HookExecution]
    """

    return (
        await asyncio_detailed(
            hook_id=hook_id,
            client=client,
        )
    ).parsed
