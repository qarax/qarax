from http import HTTPStatus
from typing import Any, cast
from uuid import UUID

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.storage_object import StorageObject
from ...models.storage_object_type import StorageObjectType
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    name: None | str | Unset = UNSET,
    pool_id: None | Unset | UUID = UNSET,
    object_type: None | StorageObjectType | Unset = UNSET,
) -> dict[str, Any]:
    params: dict[str, Any] = {}

    json_name: None | str | Unset
    if isinstance(name, Unset):
        json_name = UNSET
    else:
        json_name = name
    params["name"] = json_name

    json_pool_id: None | str | Unset
    if isinstance(pool_id, Unset):
        json_pool_id = UNSET
    elif isinstance(pool_id, UUID):
        json_pool_id = str(pool_id)
    else:
        json_pool_id = pool_id
    params["pool_id"] = json_pool_id

    json_object_type: None | str | Unset
    if isinstance(object_type, Unset):
        json_object_type = UNSET
    elif isinstance(object_type, StorageObjectType):
        json_object_type = object_type.value
    else:
        json_object_type = object_type
    params["object_type"] = json_object_type

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/storage-objects",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | list[StorageObject] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = StorageObject.from_dict(response_200_item_data)

            response_200.append(response_200_item)

        return response_200

    if response.status_code == 500:
        response_500 = cast(Any, None)
        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | list[StorageObject]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
    pool_id: None | Unset | UUID = UNSET,
    object_type: None | StorageObjectType | Unset = UNSET,
) -> Response[Any | list[StorageObject]]:
    """
    Args:
        name (None | str | Unset):
        pool_id (None | Unset | UUID):
        object_type (None | StorageObjectType | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[StorageObject]]
    """

    kwargs = _get_kwargs(
        name=name,
        pool_id=pool_id,
        object_type=object_type,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
    pool_id: None | Unset | UUID = UNSET,
    object_type: None | StorageObjectType | Unset = UNSET,
) -> Any | list[StorageObject] | None:
    """
    Args:
        name (None | str | Unset):
        pool_id (None | Unset | UUID):
        object_type (None | StorageObjectType | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[StorageObject]
    """

    return sync_detailed(
        client=client,
        name=name,
        pool_id=pool_id,
        object_type=object_type,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
    pool_id: None | Unset | UUID = UNSET,
    object_type: None | StorageObjectType | Unset = UNSET,
) -> Response[Any | list[StorageObject]]:
    """
    Args:
        name (None | str | Unset):
        pool_id (None | Unset | UUID):
        object_type (None | StorageObjectType | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[StorageObject]]
    """

    kwargs = _get_kwargs(
        name=name,
        pool_id=pool_id,
        object_type=object_type,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
    pool_id: None | Unset | UUID = UNSET,
    object_type: None | StorageObjectType | Unset = UNSET,
) -> Any | list[StorageObject] | None:
    """
    Args:
        name (None | str | Unset):
        pool_id (None | Unset | UUID):
        object_type (None | StorageObjectType | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[StorageObject]
    """

    return (
        await asyncio_detailed(
            client=client,
            name=name,
            pool_id=pool_id,
            object_type=object_type,
        )
    ).parsed
