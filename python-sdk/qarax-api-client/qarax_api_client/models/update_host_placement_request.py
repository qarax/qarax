from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.update_host_placement_request_placement_labels import UpdateHostPlacementRequestPlacementLabels


T = TypeVar("T", bound="UpdateHostPlacementRequest")


@_attrs_define
class UpdateHostPlacementRequest:
    """
    Attributes:
        placement_labels (UpdateHostPlacementRequestPlacementLabels | Unset): Arbitrary placement labels for scheduler
            filters and preferences.
        reservation_class (None | str | Unset): Optional reservation class this host belongs to.
    """

    placement_labels: UpdateHostPlacementRequestPlacementLabels | Unset = UNSET
    reservation_class: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        placement_labels: dict[str, Any] | Unset = UNSET
        if not isinstance(self.placement_labels, Unset):
            placement_labels = self.placement_labels.to_dict()

        reservation_class: None | str | Unset
        if isinstance(self.reservation_class, Unset):
            reservation_class = UNSET
        else:
            reservation_class = self.reservation_class

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if placement_labels is not UNSET:
            field_dict["placement_labels"] = placement_labels
        if reservation_class is not UNSET:
            field_dict["reservation_class"] = reservation_class

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        from ..models.update_host_placement_request_placement_labels import UpdateHostPlacementRequestPlacementLabels

        d = dict(src_dict)
        _placement_labels = d.pop("placement_labels", UNSET)
        placement_labels: UpdateHostPlacementRequestPlacementLabels | Unset
        if isinstance(_placement_labels, Unset):
            placement_labels = UNSET
        else:
            placement_labels = UpdateHostPlacementRequestPlacementLabels.from_dict(_placement_labels)

        def _parse_reservation_class(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reservation_class = _parse_reservation_class(d.pop("reservation_class", UNSET))

        update_host_placement_request = cls(
            placement_labels=placement_labels,
            reservation_class=reservation_class,
        )

        update_host_placement_request.additional_properties = d
        return update_host_placement_request

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
