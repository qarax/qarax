from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.network_interface import NetworkInterface
from ...models.new_vm_network import NewVmNetwork
from ...types import Response


def _get_kwargs(
    vm_id: UUID,
    *,
    body: NewVmNetwork,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/vms/{vm_id}/nics".format(
            vm_id=quote(str(vm_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | NetworkInterface | None:
    if response.status_code == 201:
        response_201 = NetworkInterface.from_dict(response.json())

        return response_201

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if response.status_code == 409:
        response_409 = cast(Any, None)
        return response_409

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
) -> Response[Any | NetworkInterface]:
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
    body: NewVmNetwork,
) -> Response[Any | NetworkInterface]:
    """
    Args:
        vm_id (UUID):
        body (NewVmNetwork): Network interface config for create-VM request. Passed to qarax-node;
            id is required.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | NetworkInterface]
    """

    kwargs = _get_kwargs(
        vm_id=vm_id,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    vm_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: NewVmNetwork,
) -> Any | NetworkInterface | None:
    """
    Args:
        vm_id (UUID):
        body (NewVmNetwork): Network interface config for create-VM request. Passed to qarax-node;
            id is required.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | NetworkInterface
    """

    return sync_detailed(
        vm_id=vm_id,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    vm_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: NewVmNetwork,
) -> Response[Any | NetworkInterface]:
    """
    Args:
        vm_id (UUID):
        body (NewVmNetwork): Network interface config for create-VM request. Passed to qarax-node;
            id is required.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | NetworkInterface]
    """

    kwargs = _get_kwargs(
        vm_id=vm_id,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    vm_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: NewVmNetwork,
) -> Any | NetworkInterface | None:
    """
    Args:
        vm_id (UUID):
        body (NewVmNetwork): Network interface config for create-VM request. Passed to qarax-node;
            id is required.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | NetworkInterface
    """

    return (
        await asyncio_detailed(
            vm_id=vm_id,
            client=client,
            body=body,
        )
    ).parsed
