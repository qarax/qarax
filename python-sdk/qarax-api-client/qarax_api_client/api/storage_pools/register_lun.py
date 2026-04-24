from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.create_disk_response import CreateDiskResponse
from ...models.register_lun_request import RegisterLunRequest
from ...types import Response


def _get_kwargs(
    pool_id: UUID,
    *,
    body: RegisterLunRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/storage-pools/{pool_id}/luns".format(
            pool_id=quote(str(pool_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | CreateDiskResponse | None:
    if response.status_code == 201:
        response_201 = CreateDiskResponse.from_dict(response.json())

        return response_201

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
) -> Response[Any | CreateDiskResponse]:
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
    body: RegisterLunRequest,
) -> Response[Any | CreateDiskResponse]:
    """Register a pre-existing LUN on an iSCSI (BLOCK) storage pool as a disk object.

     BLOCK pools do not support disk creation because LUNs are pre-provisioned
    on the target. Callers create the LUN out of band (e.g. via `targetcli`)
    and then register it with qarax so VMs can consume it.

    Args:
        pool_id (UUID):
        body (RegisterLunRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | CreateDiskResponse]
    """

    kwargs = _get_kwargs(
        pool_id=pool_id,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    pool_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: RegisterLunRequest,
) -> Any | CreateDiskResponse | None:
    """Register a pre-existing LUN on an iSCSI (BLOCK) storage pool as a disk object.

     BLOCK pools do not support disk creation because LUNs are pre-provisioned
    on the target. Callers create the LUN out of band (e.g. via `targetcli`)
    and then register it with qarax so VMs can consume it.

    Args:
        pool_id (UUID):
        body (RegisterLunRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | CreateDiskResponse
    """

    return sync_detailed(
        pool_id=pool_id,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    pool_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: RegisterLunRequest,
) -> Response[Any | CreateDiskResponse]:
    """Register a pre-existing LUN on an iSCSI (BLOCK) storage pool as a disk object.

     BLOCK pools do not support disk creation because LUNs are pre-provisioned
    on the target. Callers create the LUN out of band (e.g. via `targetcli`)
    and then register it with qarax so VMs can consume it.

    Args:
        pool_id (UUID):
        body (RegisterLunRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | CreateDiskResponse]
    """

    kwargs = _get_kwargs(
        pool_id=pool_id,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    pool_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: RegisterLunRequest,
) -> Any | CreateDiskResponse | None:
    """Register a pre-existing LUN on an iSCSI (BLOCK) storage pool as a disk object.

     BLOCK pools do not support disk creation because LUNs are pre-provisioned
    on the target. Callers create the LUN out of band (e.g. via `targetcli`)
    and then register it with qarax so VMs can consume it.

    Args:
        pool_id (UUID):
        body (RegisterLunRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | CreateDiskResponse
    """

    return (
        await asyncio_detailed(
            pool_id=pool_id,
            client=client,
            body=body,
        )
    ).parsed
