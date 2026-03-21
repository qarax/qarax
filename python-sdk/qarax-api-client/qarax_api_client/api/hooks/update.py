from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.lifecycle_hook import LifecycleHook
from ...models.update_lifecycle_hook import UpdateLifecycleHook
from ...types import Response


def _get_kwargs(
    hook_id: UUID,
    *,
    body: UpdateLifecycleHook,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "patch",
        "url": "/hooks/{hook_id}".format(
            hook_id=quote(str(hook_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | LifecycleHook | None:
    if response.status_code == 200:
        response_200 = LifecycleHook.from_dict(response.json())

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


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | LifecycleHook]:
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
    body: UpdateLifecycleHook,
) -> Response[Any | LifecycleHook]:
    """
    Args:
        hook_id (UUID):
        body (UpdateLifecycleHook):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | LifecycleHook]
    """

    kwargs = _get_kwargs(
        hook_id=hook_id,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    hook_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: UpdateLifecycleHook,
) -> Any | LifecycleHook | None:
    """
    Args:
        hook_id (UUID):
        body (UpdateLifecycleHook):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | LifecycleHook
    """

    return sync_detailed(
        hook_id=hook_id,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    hook_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: UpdateLifecycleHook,
) -> Response[Any | LifecycleHook]:
    """
    Args:
        hook_id (UUID):
        body (UpdateLifecycleHook):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | LifecycleHook]
    """

    kwargs = _get_kwargs(
        hook_id=hook_id,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    hook_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: UpdateLifecycleHook,
) -> Any | LifecycleHook | None:
    """
    Args:
        hook_id (UUID):
        body (UpdateLifecycleHook):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | LifecycleHook
    """

    return (
        await asyncio_detailed(
            hook_id=hook_id,
            client=client,
            body=body,
        )
    ).parsed
