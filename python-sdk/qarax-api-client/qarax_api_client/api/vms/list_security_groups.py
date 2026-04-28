from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.security_group import SecurityGroup
from ...types import Response


def _get_kwargs(
    vm_id: UUID,
) -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/vms/{vm_id}/security-groups".format(
            vm_id=quote(str(vm_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | list[SecurityGroup] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = SecurityGroup.from_dict(response_200_item_data)

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
) -> Response[Any | list[SecurityGroup]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    vm_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | list[SecurityGroup]]:
    """
    Args:
        vm_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[SecurityGroup]]
    """

    kwargs = _get_kwargs(
        vm_id=vm_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    vm_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | list[SecurityGroup] | None:
    """
    Args:
        vm_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[SecurityGroup]
    """

    return sync_detailed(
        vm_id=vm_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    vm_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | list[SecurityGroup]]:
    """
    Args:
        vm_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[SecurityGroup]]
    """

    kwargs = _get_kwargs(
        vm_id=vm_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    vm_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | list[SecurityGroup] | None:
    """
    Args:
        vm_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[SecurityGroup]
    """

    return (
        await asyncio_detailed(
            vm_id=vm_id,
            client=client,
        )
    ).parsed
