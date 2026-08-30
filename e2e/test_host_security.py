import httpx
import uuid

from helpers import AUTH_HEADERS, QARAX_URL


def test_host_credentials_are_not_exposed():
    response = httpx.get(f"{QARAX_URL}/hosts", headers=AUTH_HEADERS, timeout=10)
    response.raise_for_status()

    hosts = response.json()
    assert hosts, "Expected the e2e host fixture to register at least one host"
    assert all("password" not in host for host in hosts)


def test_environment_credential_reference_is_resolved_but_never_exposed():
    marker = f"credential-ref-{uuid.uuid4().hex[:8]}"
    response = httpx.post(
        f"{QARAX_URL}/hosts",
        json={
            "name": marker,
            "address": "127.0.0.1",
            "port": 1,
            "host_user": "root",
            # DATABASE_HOST is always present in the qarax E2E container.
            "credential_ref": "env://DATABASE_HOST",
        },
        headers=AUTH_HEADERS,
        timeout=10,
    )
    response.raise_for_status()
    host_id = response.text

    hosts_response = httpx.get(
        f"{QARAX_URL}/hosts", headers=AUTH_HEADERS, timeout=10
    )
    hosts_response.raise_for_status()
    serialized = hosts_response.text
    assert "credential_ref" not in serialized
    assert "env://DATABASE_HOST" not in serialized

    # Resolution happens synchronously before the background SSH connection.
    deploy_response = httpx.post(
        f"{QARAX_URL}/hosts/{host_id}/deploy",
        json={"image": "example.invalid/qarax:test", "reboot": False},
        headers=AUTH_HEADERS,
        timeout=10,
    )
    assert deploy_response.status_code == 202
