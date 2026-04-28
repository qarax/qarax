from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.configure_sandbox_pool_request import ConfigureSandboxPoolRequest
from ...models.sandbox_pool import SandboxPool
from ...types import Response


def _get_kwargs(
    vm_template_id: UUID,
    *,
    body: ConfigureSandboxPoolRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "put",
        "url": "/vm-templates/{vm_template_id}/sandbox-pool".format(
            vm_template_id=quote(str(vm_template_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | SandboxPool | None:
    if response.status_code == 200:
        response_200 = SandboxPool.from_dict(response.json())

        return response_200

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


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | SandboxPool]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    vm_template_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ConfigureSandboxPoolRequest,
) -> Response[Any | SandboxPool]:
    """
    Args:
        vm_template_id (UUID):
        body (ConfigureSandboxPoolRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | SandboxPool]
    """

    kwargs = _get_kwargs(
        vm_template_id=vm_template_id,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    vm_template_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ConfigureSandboxPoolRequest,
) -> Any | SandboxPool | None:
    """
    Args:
        vm_template_id (UUID):
        body (ConfigureSandboxPoolRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | SandboxPool
    """

    return sync_detailed(
        vm_template_id=vm_template_id,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    vm_template_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ConfigureSandboxPoolRequest,
) -> Response[Any | SandboxPool]:
    """
    Args:
        vm_template_id (UUID):
        body (ConfigureSandboxPoolRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | SandboxPool]
    """

    kwargs = _get_kwargs(
        vm_template_id=vm_template_id,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    vm_template_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ConfigureSandboxPoolRequest,
) -> Any | SandboxPool | None:
    """
    Args:
        vm_template_id (UUID):
        body (ConfigureSandboxPoolRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | SandboxPool
    """

    return (
        await asyncio_detailed(
            vm_template_id=vm_template_id,
            client=client,
            body=body,
        )
    ).parsed
