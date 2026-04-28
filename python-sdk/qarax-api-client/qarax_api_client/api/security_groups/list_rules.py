from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.security_group_rule import SecurityGroupRule
from ...types import Response


def _get_kwargs(
    security_group_id: UUID,
) -> dict[str, Any]:
    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/security-groups/{security_group_id}/rules".format(
            security_group_id=quote(str(security_group_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | list[SecurityGroupRule] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = SecurityGroupRule.from_dict(response_200_item_data)

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
) -> Response[Any | list[SecurityGroupRule]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    security_group_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | list[SecurityGroupRule]]:
    """
    Args:
        security_group_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[SecurityGroupRule]]
    """

    kwargs = _get_kwargs(
        security_group_id=security_group_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    security_group_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | list[SecurityGroupRule] | None:
    """
    Args:
        security_group_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[SecurityGroupRule]
    """

    return sync_detailed(
        security_group_id=security_group_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    security_group_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | list[SecurityGroupRule]]:
    """
    Args:
        security_group_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[SecurityGroupRule]]
    """

    kwargs = _get_kwargs(
        security_group_id=security_group_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    security_group_id: UUID,
    *,
    client: AuthenticatedClient | Client,
) -> Any | list[SecurityGroupRule] | None:
    """
    Args:
        security_group_id (UUID):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[SecurityGroupRule]
    """

    return (
        await asyncio_detailed(
            security_group_id=security_group_id,
            client=client,
        )
    ).parsed
