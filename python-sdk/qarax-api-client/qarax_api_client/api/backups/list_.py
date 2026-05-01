from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.backup import Backup
from ...models.backup_type import BackupType
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    name: None | str | Unset = UNSET,
    backup_type: BackupType | None | Unset = UNSET,
) -> dict[str, Any]:
    params: dict[str, Any] = {}

    json_name: None | str | Unset
    if isinstance(name, Unset):
        json_name = UNSET
    else:
        json_name = name
    params["name"] = json_name

    json_backup_type: None | str | Unset
    if isinstance(backup_type, Unset):
        json_backup_type = UNSET
    elif isinstance(backup_type, BackupType):
        json_backup_type = backup_type.value
    else:
        json_backup_type = backup_type
    params["backup_type"] = json_backup_type

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/backups",
        "params": params,
    }

    return _kwargs


def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | list[Backup] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = Backup.from_dict(response_200_item_data)

            response_200.append(response_200_item)

        return response_200

    if response.status_code == 500:
        response_500 = cast(Any, None)
        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | list[Backup]]:
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
    backup_type: BackupType | None | Unset = UNSET,
) -> Response[Any | list[Backup]]:
    """
    Args:
        name (None | str | Unset):
        backup_type (BackupType | None | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[Backup]]
    """

    kwargs = _get_kwargs(
        name=name,
        backup_type=backup_type,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
    backup_type: BackupType | None | Unset = UNSET,
) -> Any | list[Backup] | None:
    """
    Args:
        name (None | str | Unset):
        backup_type (BackupType | None | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[Backup]
    """

    return sync_detailed(
        client=client,
        name=name,
        backup_type=backup_type,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
    backup_type: BackupType | None | Unset = UNSET,
) -> Response[Any | list[Backup]]:
    """
    Args:
        name (None | str | Unset):
        backup_type (BackupType | None | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | list[Backup]]
    """

    kwargs = _get_kwargs(
        name=name,
        backup_type=backup_type,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    name: None | str | Unset = UNSET,
    backup_type: BackupType | None | Unset = UNSET,
) -> Any | list[Backup] | None:
    """
    Args:
        name (None | str | Unset):
        backup_type (BackupType | None | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | list[Backup]
    """

    return (
        await asyncio_detailed(
            client=client,
            name=name,
            backup_type=backup_type,
        )
    ).parsed
