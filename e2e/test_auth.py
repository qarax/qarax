"""E2E tests for API token authentication."""

import httpx
import pytest

from helpers import AUTH_HEADERS, QARAX_URL


def test_protected_endpoint_requires_auth():
    """GET /hosts without an Authorization header is rejected."""
    resp = httpx.get(f"{QARAX_URL}/hosts")
    assert resp.status_code == 401


def test_protected_endpoint_rejects_wrong_token():
    """GET /hosts with an incorrect bearer token is rejected."""
    resp = httpx.get(f"{QARAX_URL}/hosts", headers={"Authorization": "Bearer not-the-right-token"})
    assert resp.status_code == 401


def test_protected_endpoint_accepts_valid_token():
    """GET /hosts with the configured bearer token succeeds."""
    resp = httpx.get(f"{QARAX_URL}/hosts", headers=AUTH_HEADERS)
    assert resp.status_code == 200


@pytest.mark.parametrize(
    "path",
    ["/", "/swagger-ui", "/api-docs/openapi.json"],
)
def test_public_paths_do_not_require_auth(path):
    """Health check, Swagger UI, and the OpenAPI spec remain accessible without auth."""
    resp = httpx.get(f"{QARAX_URL}{path}", follow_redirects=True)
    assert resp.status_code == 200
