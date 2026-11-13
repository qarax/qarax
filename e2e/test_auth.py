"""E2E tests for API token authentication."""

import httpx

from helpers import QARAX_URL


def test_protected_endpoint_requires_auth():
    """GET /hosts without an Authorization header is rejected."""
    resp = httpx.get(f"{QARAX_URL}/hosts")
    assert resp.status_code == 401
