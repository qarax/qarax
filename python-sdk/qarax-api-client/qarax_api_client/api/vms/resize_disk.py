from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.disk_resize_request import DiskResizeRequest
from ...models.storage_object import StorageObject
from ...types import Response


def _get_kwargs(
    vm_id: UUID,
    disk_id: str,
    *,
    body: DiskResizeRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "put",
        "url": "/vms/{vm_id}/disks/{disk_id}/resize".format(
            vm_id=quote(str(vm_id), safe=""),
            disk_id=quote(str(disk_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | StorageObject | None:
    if response.status_code == 200:
        response_200 = StorageObject.from_dict(response.json())

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


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | StorageObject]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    vm_id: UUID,
    disk_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: DiskResizeRequest,
) -> Response[Any | StorageObject]:
    """
    Args:
        vm_id (UUID):
        disk_id (str):
        body (DiskResizeRequest): Request body for `PUT /vms/{vm_id}/disks/{disk_id}/resize`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | StorageObject]
    """

    kwargs = _get_kwargs(
        vm_id=vm_id,
        disk_id=disk_id,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    vm_id: UUID,
    disk_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: DiskResizeRequest,
) -> Any | StorageObject | None:
    """
    Args:
        vm_id (UUID):
        disk_id (str):
        body (DiskResizeRequest): Request body for `PUT /vms/{vm_id}/disks/{disk_id}/resize`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | StorageObject
    """

    return sync_detailed(
        vm_id=vm_id,
        disk_id=disk_id,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    vm_id: UUID,
    disk_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: DiskResizeRequest,
) -> Response[Any | StorageObject]:
    """
    Args:
        vm_id (UUID):
        disk_id (str):
        body (DiskResizeRequest): Request body for `PUT /vms/{vm_id}/disks/{disk_id}/resize`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | StorageObject]
    """

    kwargs = _get_kwargs(
        vm_id=vm_id,
        disk_id=disk_id,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    vm_id: UUID,
    disk_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: DiskResizeRequest,
) -> Any | StorageObject | None:
    """
    Args:
        vm_id (UUID):
        disk_id (str):
        body (DiskResizeRequest): Request body for `PUT /vms/{vm_id}/disks/{disk_id}/resize`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | StorageObject
    """

    return (
        await asyncio_detailed(
            vm_id=vm_id,
            disk_id=disk_id,
            client=client,
            body=body,
        )
    ).parsed
