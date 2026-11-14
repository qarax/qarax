from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="NewBootSource")


@_attrs_define
class NewBootSource:
    """
    Attributes:
        kernel_image_id (UUID):
        name (str):
        description (None | str | Unset):
        initrd_image_id (None | Unset | UUID):
        kernel_params (None | str | Unset):
    """

    kernel_image_id: UUID
    name: str
    description: None | str | Unset = UNSET
    initrd_image_id: None | Unset | UUID = UNSET
    kernel_params: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kernel_image_id = str(self.kernel_image_id)

        name = self.name

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        initrd_image_id: None | str | Unset
        if isinstance(self.initrd_image_id, Unset):
            initrd_image_id = UNSET
        elif isinstance(self.initrd_image_id, UUID):
            initrd_image_id = str(self.initrd_image_id)
        else:
            initrd_image_id = self.initrd_image_id

        kernel_params: None | str | Unset
        if isinstance(self.kernel_params, Unset):
            kernel_params = UNSET
        else:
            kernel_params = self.kernel_params

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "kernel_image_id": kernel_image_id,
                "name": name,
            }
        )
        if description is not UNSET:
            field_dict["description"] = description
        if initrd_image_id is not UNSET:
            field_dict["initrd_image_id"] = initrd_image_id
        if kernel_params is not UNSET:
            field_dict["kernel_params"] = kernel_params

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        kernel_image_id = UUID(d.pop("kernel_image_id"))

        name = d.pop("name")

        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))

        def _parse_initrd_image_id(data: object) -> None | Unset | UUID:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                initrd_image_id_type_0 = UUID(data)

                return initrd_image_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | UUID, data)

        initrd_image_id = _parse_initrd_image_id(d.pop("initrd_image_id", UNSET))

        def _parse_kernel_params(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        kernel_params = _parse_kernel_params(d.pop("kernel_params", UNSET))

        new_boot_source = cls(
            kernel_image_id=kernel_image_id,
            name=name,
            description=description,
            initrd_image_id=initrd_image_id,
            kernel_params=kernel_params,
        )

        new_boot_source.additional_properties = d
        return new_boot_source

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
