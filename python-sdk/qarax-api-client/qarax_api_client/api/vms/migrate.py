from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.vm_migrate_request import VmMigrateRequest
from ...models.vm_migrate_response import VmMigrateResponse
from ...types import Response


def _get_kwargs(
    vm_id: UUID,
    *,
    body: VmMigrateRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/vms/{vm_id}/migrate".format(
            vm_id=quote(str(vm_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | VmMigrateResponse | None:
    if response.status_code == 202:
        response_202 = VmMigrateResponse.from_dict(response.json())

        return response_202

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
) -> Response[Any | VmMigrateResponse]:
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
    body: VmMigrateRequest,
) -> Response[Any | VmMigrateResponse]:
    """
    Args:
        vm_id (UUID):
        body (VmMigrateRequest): Request body for `POST /vms/{vm_id}/migrate`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | VmMigrateResponse]
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
    body: VmMigrateRequest,
) -> Any | VmMigrateResponse | None:
    """
    Args:
        vm_id (UUID):
        body (VmMigrateRequest): Request body for `POST /vms/{vm_id}/migrate`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | VmMigrateResponse
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
    body: VmMigrateRequest,
) -> Response[Any | VmMigrateResponse]:
    """
    Args:
        vm_id (UUID):
        body (VmMigrateRequest): Request body for `POST /vms/{vm_id}/migrate`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | VmMigrateResponse]
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
    body: VmMigrateRequest,
) -> Any | VmMigrateResponse | None:
    """
    Args:
        vm_id (UUID):
        body (VmMigrateRequest): Request body for `POST /vms/{vm_id}/migrate`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | VmMigrateResponse
    """

    return (
        await asyncio_detailed(
            vm_id=vm_id,
            client=client,
            body=body,
        )
    ).parsed
