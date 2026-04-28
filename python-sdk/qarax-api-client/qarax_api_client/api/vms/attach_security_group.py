from http import HTTPStatus
from typing import Any
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.attach_security_group_request import AttachSecurityGroupRequest
from ...types import Response


def _get_kwargs(
    vm_id: UUID,
    *,
    body: AttachSecurityGroupRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/vms/{vm_id}/security-groups".format(
            vm_id=quote(str(vm_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | None:
    if response.status_code == 204:
        return None

    if response.status_code == 404:
        return None

    if response.status_code == 500:
        return None

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any]:
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
    body: AttachSecurityGroupRequest,
) -> Response[Any]:
    """
    Args:
        vm_id (UUID):
        body (AttachSecurityGroupRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any]
    """

    kwargs = _get_kwargs(
        vm_id=vm_id,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


async def asyncio_detailed(
    vm_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: AttachSecurityGroupRequest,
) -> Response[Any]:
    """
    Args:
        vm_id (UUID):
        body (AttachSecurityGroupRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any]
    """

    kwargs = _get_kwargs(
        vm_id=vm_id,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)
