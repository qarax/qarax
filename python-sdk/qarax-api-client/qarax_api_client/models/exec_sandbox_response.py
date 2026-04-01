from __future__ import annotations

from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ExecSandboxResponse")


@_attrs_define
class ExecSandboxResponse:
    """
    Attributes:
        exit_code (int):
        stderr (str):
        stdout (str):
        timed_out (bool):
    """

    exit_code: int
    stderr: str
    stdout: str
    timed_out: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        exit_code = self.exit_code

        stderr = self.stderr

        stdout = self.stdout

        timed_out = self.timed_out

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "exit_code": exit_code,
                "stderr": stderr,
                "stdout": stdout,
                "timed_out": timed_out,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Any) -> T:
        d = dict(src_dict)
        exit_code = d.pop("exit_code")

        stderr = d.pop("stderr")

        stdout = d.pop("stdout")

        timed_out = d.pop("timed_out")

        exec_sandbox_response = cls(
            exit_code=exit_code,
            stderr=stderr,
            stdout=stdout,
            timed_out=timed_out,
        )

        exec_sandbox_response.additional_properties = d
        return exec_sandbox_response

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
