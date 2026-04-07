"""
E2E tests for the audit log feature.

These tests verify that audit log entries are created when VM and host
lifecycle operations are performed.
"""

import os
import uuid

import httpx
import pytest

QARAX_URL = os.getenv("QARAX_URL", "http://localhost:8000")


def list_audit_logs(
    resource_type: str | None = None,
    resource_id: str | None = None,
    action: str | None = None,
    limit: int | None = None,
) -> list[dict]:
    params = {}
    if resource_type:
        params["resource_type"] = resource_type
    if resource_id:
        params["resource_id"] = resource_id
    if action:
        params["action"] = action
    if limit is not None:
        params["limit"] = limit
    resp = httpx.get(f"{QARAX_URL}/audit-logs", params=params)
    resp.raise_for_status()
    return resp.json()


def create_host() -> dict[str, str]:
    suffix = uuid.uuid4().hex[:8]
    host = {
        "name": f"audit-host-{suffix}",
        "address": f"192.0.2.{int(suffix[:2], 16) % 250 + 1}",
        "port": 50051,
        "host_user": "root",
        "password": "",
    }
    resp = httpx.post(f"{QARAX_URL}/hosts", json=host)
    resp.raise_for_status()
    return {"id": resp.text.strip(), "name": host["name"]}


def get_audit_log(audit_log_id: str) -> dict:
    resp = httpx.get(f"{QARAX_URL}/audit-logs/{audit_log_id}")
    resp.raise_for_status()
    return resp.json()


def test_audit_logs_endpoint_returns_list():
    """GET /audit-logs returns a list (may be empty)."""
    logs = list_audit_logs()
    assert isinstance(logs, list)


def test_audit_log_list_has_expected_fields():
    """Each audit log entry has the required fields."""
    logs = list_audit_logs(limit=1)
    if not logs:
        pytest.skip("No audit log entries yet")
    log = logs[0]
    assert "id" in log
    assert "action" in log
    assert "resource_type" in log
    assert "resource_id" in log
    assert "created_at" in log


def test_audit_log_get_single():
    """GET /audit-logs/{id} returns a single entry matching the ID."""
    host = create_host()
    logs = list_audit_logs(
        resource_type="host",
        resource_id=host["id"],
        action="create",
        limit=1,
    )
    assert len(logs) == 1
    log_id = logs[0]["id"]
    log = get_audit_log(log_id)
    assert log["id"] == log_id
    assert log["resource_type"] == "host"
    assert log["resource_id"] == host["id"]
    assert log["resource_name"] == host["name"]


def test_host_creation_is_audited():
    """Adding a host creates an audit log entry with action=create and resource_type=host."""
    host = create_host()
    logs = list_audit_logs(
        resource_type="host",
        resource_id=host["id"],
        action="create",
    )
    assert len(logs) == 1, "Expected exactly one matching host create audit log"
    entry = logs[0]
    assert entry["action"] == "create"
    assert entry["resource_type"] == "host"
    assert entry["resource_id"] == host["id"]
    assert entry["resource_name"] == host["name"]


def test_audit_log_filter_by_resource_id():
    """Filtering by resource_id returns only matching entries."""
    host = create_host()
    filtered = list_audit_logs(resource_id=host["id"])
    assert filtered
    assert any(log["resource_name"] == host["name"] for log in filtered)
    assert all(log["resource_id"] == host["id"] for log in filtered)


def test_audit_log_filter_by_action():
    """Filtering by action returns only matching entries."""
    host = create_host()
    logs = list_audit_logs(action="create")
    assert any(log["resource_id"] == host["id"] for log in logs)
    assert all(log["action"] == "create" for log in logs)


def test_audit_log_limit_param():
    """The limit parameter is respected."""
    logs = list_audit_logs(limit=1)
    assert len(logs) <= 1


def test_audit_log_get_not_found():
    """GET /audit-logs/{id} returns 404 for a non-existent ID."""
    non_existent = "00000000-0000-0000-0000-000000000000"
    resp = httpx.get(f"{QARAX_URL}/audit-logs/{non_existent}")
    assert resp.status_code == 404


def test_audit_log_invalid_action_filter_returns_400():
    """Invalid action filters should be rejected instead of ignored."""
    resp = httpx.get(f"{QARAX_URL}/audit-logs", params={"action": "not-a-real-action"})
    assert resp.status_code == 400
