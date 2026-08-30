from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.new_host_placement_labels import NewHostPlacementLabels


T = TypeVar("T", bound="NewHost")


@_attrs_define
class NewHost:
    """
    Attributes:
        address (str):
        host_user (str):
        name (str):
        port (int):
        credential_ref (None | str | Unset): External SSH password reference (`env://NAME` or `file:///absolute/path`).
            Accepted on input but never returned by host APIs.
        placement_labels (NewHostPlacementLabels | Unset): Arbitrary placement labels for scheduler filters and
            preferences.
        reservation_class (None | str | Unset): Optional reservation class this host belongs to.
    """

    address: str
    host_user: str
    name: str
    port: int
    credential_ref: None | str | Unset = UNSET
    placement_labels: NewHostPlacementLabels | Unset = UNSET
    reservation_class: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        address = self.address

        host_user = self.host_user

        name = self.name

        port = self.port

        credential_ref: None | str | Unset
        if isinstance(self.credential_ref, Unset):
            credential_ref = UNSET
        else:
            credential_ref = self.credential_ref

        placement_labels: dict[str, Any] | Unset = UNSET
        if not isinstance(self.placement_labels, Unset):
            placement_labels = self.placement_labels.to_dict()

        reservation_class: None | str | Unset
        if isinstance(self.reservation_class, Unset):
            reservation_class = UNSET
        else:
            reservation_class = self.reservation_class

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "address": address,
                "host_user": host_user,
                "name": name,
                "port": port,
            }
        )
        if credential_ref is not UNSET:
            field_dict["credential_ref"] = credential_ref
        if placement_labels is not UNSET:
            field_dict["placement_labels"] = placement_labels
        if reservation_class is not UNSET:
            field_dict["reservation_class"] = reservation_class

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        from ..models.new_host_placement_labels import NewHostPlacementLabels

        d = dict(src_dict)
        address = d.pop("address")

        host_user = d.pop("host_user")

        name = d.pop("name")

        port = d.pop("port")

        def _parse_credential_ref(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        credential_ref = _parse_credential_ref(d.pop("credential_ref", UNSET))

        _placement_labels = d.pop("placement_labels", UNSET)
        placement_labels: NewHostPlacementLabels | Unset
        if isinstance(_placement_labels, Unset):
            placement_labels = UNSET
        else:
            placement_labels = NewHostPlacementLabels.from_dict(_placement_labels)

        def _parse_reservation_class(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reservation_class = _parse_reservation_class(d.pop("reservation_class", UNSET))

        new_host = cls(
            address=address,
            host_user=host_user,
            name=name,
            port=port,
            credential_ref=credential_ref,
            placement_labels=placement_labels,
            reservation_class=reservation_class,
        )

        return new_host
