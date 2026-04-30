from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.placement_policy_preferred_host_labels import PlacementPolicyPreferredHostLabels
    from ..models.placement_policy_required_host_labels import PlacementPolicyRequiredHostLabels


T = TypeVar("T", bound="PlacementPolicy")


@_attrs_define
class PlacementPolicy:
    """
    Attributes:
        affinity_tags (list[str] | Unset): Prefer hosts already running active VMs that have all of these tags.
        anti_affinity_tags (list[str] | Unset): Exclude hosts running active VMs that have any of these tags.
        preferred_host_labels (PlacementPolicyPreferredHostLabels | Unset): Soft host-label preference. Hosts matching
            all labels sort ahead of others.
        required_host_labels (PlacementPolicyRequiredHostLabels | Unset): Hard host-label filter. Every listed label
            must exist on the host.
        reservation_class (None | str | Unset): Require placement on hosts from this reservation class.
        spread_tags (list[str] | Unset): Prefer hosts with fewer active VMs that have all of these tags.
    """

    affinity_tags: list[str] | Unset = UNSET
    anti_affinity_tags: list[str] | Unset = UNSET
    preferred_host_labels: PlacementPolicyPreferredHostLabels | Unset = UNSET
    required_host_labels: PlacementPolicyRequiredHostLabels | Unset = UNSET
    reservation_class: None | str | Unset = UNSET
    spread_tags: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        affinity_tags: list[str] | Unset = UNSET
        if not isinstance(self.affinity_tags, Unset):
            affinity_tags = self.affinity_tags

        anti_affinity_tags: list[str] | Unset = UNSET
        if not isinstance(self.anti_affinity_tags, Unset):
            anti_affinity_tags = self.anti_affinity_tags

        preferred_host_labels: dict[str, Any] | Unset = UNSET
        if not isinstance(self.preferred_host_labels, Unset):
            preferred_host_labels = self.preferred_host_labels.to_dict()

        required_host_labels: dict[str, Any] | Unset = UNSET
        if not isinstance(self.required_host_labels, Unset):
            required_host_labels = self.required_host_labels.to_dict()

        reservation_class: None | str | Unset
        if isinstance(self.reservation_class, Unset):
            reservation_class = UNSET
        else:
            reservation_class = self.reservation_class

        spread_tags: list[str] | Unset = UNSET
        if not isinstance(self.spread_tags, Unset):
            spread_tags = self.spread_tags

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if affinity_tags is not UNSET:
            field_dict["affinity_tags"] = affinity_tags
        if anti_affinity_tags is not UNSET:
            field_dict["anti_affinity_tags"] = anti_affinity_tags
        if preferred_host_labels is not UNSET:
            field_dict["preferred_host_labels"] = preferred_host_labels
        if required_host_labels is not UNSET:
            field_dict["required_host_labels"] = required_host_labels
        if reservation_class is not UNSET:
            field_dict["reservation_class"] = reservation_class
        if spread_tags is not UNSET:
            field_dict["spread_tags"] = spread_tags

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        from ..models.placement_policy_preferred_host_labels import PlacementPolicyPreferredHostLabels
        from ..models.placement_policy_required_host_labels import PlacementPolicyRequiredHostLabels

        d = dict(src_dict)
        affinity_tags = cast(list[str], d.pop("affinity_tags", UNSET))

        anti_affinity_tags = cast(list[str], d.pop("anti_affinity_tags", UNSET))

        _preferred_host_labels = d.pop("preferred_host_labels", UNSET)
        preferred_host_labels: PlacementPolicyPreferredHostLabels | Unset
        if isinstance(_preferred_host_labels, Unset):
            preferred_host_labels = UNSET
        else:
            preferred_host_labels = PlacementPolicyPreferredHostLabels.from_dict(_preferred_host_labels)

        _required_host_labels = d.pop("required_host_labels", UNSET)
        required_host_labels: PlacementPolicyRequiredHostLabels | Unset
        if isinstance(_required_host_labels, Unset):
            required_host_labels = UNSET
        else:
            required_host_labels = PlacementPolicyRequiredHostLabels.from_dict(_required_host_labels)

        def _parse_reservation_class(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reservation_class = _parse_reservation_class(d.pop("reservation_class", UNSET))

        spread_tags = cast(list[str], d.pop("spread_tags", UNSET))

        placement_policy = cls(
            affinity_tags=affinity_tags,
            anti_affinity_tags=anti_affinity_tags,
            preferred_host_labels=preferred_host_labels,
            required_host_labels=required_host_labels,
            reservation_class=reservation_class,
            spread_tags=spread_tags,
        )

        placement_policy.additional_properties = d
        return placement_policy

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
