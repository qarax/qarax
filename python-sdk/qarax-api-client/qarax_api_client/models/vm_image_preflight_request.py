from __future__ import annotations

from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.boot_mode import BootMode
from ..types import UNSET, Unset

T = TypeVar("T", bound="VmImagePreflightRequest")


@_attrs_define
class VmImagePreflightRequest:
    """
    Attributes:
        image_ref (str):
        architecture (None | str | Unset):
        boot_mode (BootMode | None | Unset):
        host_id (None | Unset | UUID):
    """

    image_ref: str
    architecture: None | str | Unset = UNSET
    boot_mode: BootMode | None | Unset = UNSET
    host_id: None | Unset | UUID = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        image_ref = self.image_ref

        architecture: None | str | Unset
        if isinstance(self.architecture, Unset):
            architecture = UNSET
        else:
            architecture = self.architecture

        boot_mode: None | str | Unset
        if isinstance(self.boot_mode, Unset):
            boot_mode = UNSET
        elif isinstance(self.boot_mode, BootMode):
            boot_mode = self.boot_mode.value
        else:
            boot_mode = self.boot_mode

        host_id: None | str | Unset
        if isinstance(self.host_id, Unset):
            host_id = UNSET
        elif isinstance(self.host_id, UUID):
            host_id = str(self.host_id)
        else:
            host_id = self.host_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "image_ref": image_ref,
            }
        )
        if architecture is not UNSET:
            field_dict["architecture"] = architecture
        if boot_mode is not UNSET:
            field_dict["boot_mode"] = boot_mode
        if host_id is not UNSET:
            field_dict["host_id"] = host_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        image_ref = d.pop("image_ref")

        def _parse_architecture(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        architecture = _parse_architecture(d.pop("architecture", UNSET))

        def _parse_boot_mode(data: object) -> BootMode | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                boot_mode_type_1 = BootMode(data)

                return boot_mode_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BootMode | None | Unset, data)

        boot_mode = _parse_boot_mode(d.pop("boot_mode", UNSET))

        def _parse_host_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                host_id_type_0 = UUID(data)

                return host_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        host_id = _parse_host_id(d.pop("host_id", UNSET))

        vm_image_preflight_request = cls(
            image_ref=image_ref,
            architecture=architecture,
            boot_mode=boot_mode,
            host_id=host_id,
        )

        vm_image_preflight_request.additional_properties = d
        return vm_image_preflight_request

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
