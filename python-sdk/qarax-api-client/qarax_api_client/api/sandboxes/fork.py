from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx2

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.fork_sandbox_request import ForkSandboxRequest
from ...models.sandbox import Sandbox
from ...types import Response


def _get_kwargs(
    sandbox_id: UUID,
    *,
    body: ForkSandboxRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/sandboxes/{sandbox_id}/fork".format(
            sandbox_id=quote(str(sandbox_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx2.Response) -> Any | list[Sandbox] | None:
    if response.status_code == 201:
        response_201 = []
        _response_201 = response.json()
        for response_201_item_data in _response_201:
            response_201_item = Sandbox.from_dict(response_201_item_data)

            response_201.append(response_201_item)

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
    *, client: AuthenticatedClient | Client, response: httpx2.Response
) -> Response[Any | list[Sandbox]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    sandbox_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ForkSandboxRequest,
) -> Response[Any | list[Sandbox]]:
    """Fork a running sandbox

     Makes `count` new sandboxes that continue from this one's current state:
    its memory, its running processes and its files. They run on the same host
    and are ready when this returns; this one keeps running. Each fork's random
    generator is reseeded and its clock set. A fork of a sandbox booted from an
    OCI image gets the image and the sandbox's changes to it as they were, and a
    copy of each of its writable disks on local and NFS pools, of its own;
    read-only disks are shared. Each of a fork's network interfaces is on its
    source's network with an address of its own, and the fork has its source's
    security groups; a sandbox's cloud-init seed disk is each fork's own. Only
    sandboxes with the guest agent, no network interface outside a managed
    bridge or isolated network (nor one whose network config comes from
    cloud-init) and no writable disk on another kind of pool fork.

    Args:
        sandbox_id (UUID):
        body (ForkSandboxRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx2.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[Sandbox]]
    """

    kwargs = _get_kwargs(
        sandbox_id=sandbox_id,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    sandbox_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ForkSandboxRequest,
) -> Any | list[Sandbox] | None:
    """Fork a running sandbox

     Makes `count` new sandboxes that continue from this one's current state:
    its memory, its running processes and its files. They run on the same host
    and are ready when this returns; this one keeps running. Each fork's random
    generator is reseeded and its clock set. A fork of a sandbox booted from an
    OCI image gets the image and the sandbox's changes to it as they were, and a
    copy of each of its writable disks on local and NFS pools, of its own;
    read-only disks are shared. Each of a fork's network interfaces is on its
    source's network with an address of its own, and the fork has its source's
    security groups; a sandbox's cloud-init seed disk is each fork's own. Only
    sandboxes with the guest agent, no network interface outside a managed
    bridge or isolated network (nor one whose network config comes from
    cloud-init) and no writable disk on another kind of pool fork.

    Args:
        sandbox_id (UUID):
        body (ForkSandboxRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx2.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[Sandbox]
    """

    return sync_detailed(
        sandbox_id=sandbox_id,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    sandbox_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ForkSandboxRequest,
) -> Response[Any | list[Sandbox]]:
    """Fork a running sandbox

     Makes `count` new sandboxes that continue from this one's current state:
    its memory, its running processes and its files. They run on the same host
    and are ready when this returns; this one keeps running. Each fork's random
    generator is reseeded and its clock set. A fork of a sandbox booted from an
    OCI image gets the image and the sandbox's changes to it as they were, and a
    copy of each of its writable disks on local and NFS pools, of its own;
    read-only disks are shared. Each of a fork's network interfaces is on its
    source's network with an address of its own, and the fork has its source's
    security groups; a sandbox's cloud-init seed disk is each fork's own. Only
    sandboxes with the guest agent, no network interface outside a managed
    bridge or isolated network (nor one whose network config comes from
    cloud-init) and no writable disk on another kind of pool fork.

    Args:
        sandbox_id (UUID):
        body (ForkSandboxRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx2.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[Sandbox]]
    """

    kwargs = _get_kwargs(
        sandbox_id=sandbox_id,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    sandbox_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ForkSandboxRequest,
) -> Any | list[Sandbox] | None:
    """Fork a running sandbox

     Makes `count` new sandboxes that continue from this one's current state:
    its memory, its running processes and its files. They run on the same host
    and are ready when this returns; this one keeps running. Each fork's random
    generator is reseeded and its clock set. A fork of a sandbox booted from an
    OCI image gets the image and the sandbox's changes to it as they were, and a
    copy of each of its writable disks on local and NFS pools, of its own;
    read-only disks are shared. Each of a fork's network interfaces is on its
    source's network with an address of its own, and the fork has its source's
    security groups; a sandbox's cloud-init seed disk is each fork's own. Only
    sandboxes with the guest agent, no network interface outside a managed
    bridge or isolated network (nor one whose network config comes from
    cloud-init) and no writable disk on another kind of pool fork.

    Args:
        sandbox_id (UUID):
        body (ForkSandboxRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx2.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[Sandbox]
    """

    return (
        await asyncio_detailed(
            sandbox_id=sandbox_id,
            client=client,
            body=body,
        )
    ).parsed
