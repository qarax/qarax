from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="DeployHostRequest")


@_attrs_define
class DeployHostRequest:
    """
    Attributes:
        image (str): Fully-qualified bootc image reference to deploy on the host.
        install_bootc (bool | None | Unset): Install bootc before switching image. Defaults to true.
        reboot (bool | None | Unset): Reboot after `bootc switch`. Defaults to true.
        ssh_password (None | str | Unset): Optional SSH password override for this deployment request.
        ssh_port (int | None | Unset): SSH port used to reach the host. Defaults to 22.
        ssh_private_key_path (None | str | Unset): Optional SSH private key path on the qarax control-plane host.
        ssh_user (None | str | Unset): SSH user override. Defaults to the host's registered `host_user`.
    """

    image: str
    install_bootc: bool | None | Unset = UNSET
    reboot: bool | None | Unset = UNSET
    ssh_password: None | str | Unset = UNSET
    ssh_port: int | None | Unset = UNSET
    ssh_private_key_path: None | str | Unset = UNSET
    ssh_user: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        image = self.image

        install_bootc: bool | None | Unset
        if isinstance(self.install_bootc, Unset):
            install_bootc = UNSET
        else:
            install_bootc = self.install_bootc

        reboot: bool | None | Unset
        if isinstance(self.reboot, Unset):
            reboot = UNSET
        else:
            reboot = self.reboot

        ssh_password: None | str | Unset
        if isinstance(self.ssh_password, Unset):
            ssh_password = UNSET
        else:
            ssh_password = self.ssh_password

        ssh_port: int | None | Unset
        if isinstance(self.ssh_port, Unset):
            ssh_port = UNSET
        else:
            ssh_port = self.ssh_port

        ssh_private_key_path: None | str | Unset
        if isinstance(self.ssh_private_key_path, Unset):
            ssh_private_key_path = UNSET
        else:
            ssh_private_key_path = self.ssh_private_key_path

        ssh_user: None | str | Unset
        if isinstance(self.ssh_user, Unset):
            ssh_user = UNSET
        else:
            ssh_user = self.ssh_user

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "image": image,
            }
        )
        if install_bootc is not UNSET:
            field_dict["install_bootc"] = install_bootc
        if reboot is not UNSET:
            field_dict["reboot"] = reboot
        if ssh_password is not UNSET:
            field_dict["ssh_password"] = ssh_password
        if ssh_port is not UNSET:
            field_dict["ssh_port"] = ssh_port
        if ssh_private_key_path is not UNSET:
            field_dict["ssh_private_key_path"] = ssh_private_key_path
        if ssh_user is not UNSET:
            field_dict["ssh_user"] = ssh_user

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        image = d.pop("image")

        def _parse_install_bootc(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        install_bootc = _parse_install_bootc(d.pop("install_bootc", UNSET))

        def _parse_reboot(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        reboot = _parse_reboot(d.pop("reboot", UNSET))

        def _parse_ssh_password(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ssh_password = _parse_ssh_password(d.pop("ssh_password", UNSET))

        def _parse_ssh_port(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ssh_port = _parse_ssh_port(d.pop("ssh_port", UNSET))

        def _parse_ssh_private_key_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ssh_private_key_path = _parse_ssh_private_key_path(d.pop("ssh_private_key_path", UNSET))

        def _parse_ssh_user(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ssh_user = _parse_ssh_user(d.pop("ssh_user", UNSET))

        deploy_host_request = cls(
            image=image,
            install_bootc=install_bootc,
            reboot=reboot,
            ssh_password=ssh_password,
            ssh_port=ssh_port,
            ssh_private_key_path=ssh_private_key_path,
            ssh_user=ssh_user,
        )

        deploy_host_request.additional_properties = d
        return deploy_host_request

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
